//! "dockable and resizable most left panel (Outline, Agenda, Blocks,
//! …). Currently it has a fixed width. I would like to make it
//! dockable, in order that just the icons are visible and none of the
//! text."
//!
//! And the note left on the window-configuration item: "neither is the
//! left rail's collapsed state — that one is its own item about making
//! the rail dockable."
//!
//! So two things: the rail collapses to its icons, and it stays
//! collapsed. The second is the one that matters — a toggle you have to
//! press every morning is a toggle you stop pressing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01RAILAAAAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(InputMode::Doom))
}

#[test]
fn the_rail_starts_showing_its_labels() {
    let (_d, _shell, app) = app();
    assert!(!app.rail_docked(), "a first run should read, not guess");
}

#[test]
fn one_command_docks_it_to_the_icons() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-rail");
    assert!(app.rail_docked());
    app.run(&mut shell, "toggle-rail");
    assert!(!app.rail_docked(), "the same command brings it back");
}

#[test]
fn docking_it_says_what_happened() {
    // The rail losing its labels with no word about why is the same
    // event as the rail breaking.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-rail");
    assert!(
        app.status().to_lowercase().contains("rail"),
        "no word about it: {}",
        app.status()
    );
}

#[test]
fn the_state_is_written_where_the_pane_width_is() {
    // Same mechanism as `outline_width`, which is already remembered
    // in config.org — the rail is the other half of "remember window
    // configuration".
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-rail");
    assert_eq!(app.rail_docked_setting(), Some(true));
    app.run(&mut shell, "toggle-rail");
    assert_eq!(app.rail_docked_setting(), Some(false));
}

#[test]
fn a_config_that_says_docked_opens_docked() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nrail_docked = true\n#+END_SRC\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01RAILBBBBBBBBBBBBBBBBBBBB\n:END:\n",
    )
    .unwrap();
    let cfg = closure_config::Config::from_path(&dir.path().join("config.org")).expect("config");
    assert_eq!(cfg.rail_docked, Some(true));
}

#[test]
fn a_hand_written_nonsense_value_is_ignored_rather_than_obeyed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nrail_docked = maybe\n#+END_SRC\n",
    )
    .unwrap();
    // A bad boolean is a config error, the same as every other bool
    // here — not a silent `false`.
    assert!(closure_config::Config::from_path(&dir.path().join("config.org")).is_err());
}

#[test]
fn the_chord_reaches_it_from_the_outline() {
    // `run` is not how anyone docks the rail; `g H` is. A command that
    // only works from the palette is half a feature.
    let (_d, mut shell, mut app) = app();
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "H", false, false, Some('H'));
    assert!(
        app.rail_docked(),
        "g H did nothing (status: {})",
        app.status()
    );
}

#[test]
fn the_keymap_carries_it_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let bound = closure_input::mode_keymap(mode)
            .iter()
            .any(|(_, cmd)| *cmd == "toggle-rail");
        assert!(bound, "{mode:?} cannot reach the rail at all");
    }
}

#[test]
fn closing_the_window_writes_it_into_config_org() {
    // The half that matters: a toggle you have to press every morning
    // is a toggle you stop pressing. Same writer as `outline_width`
    // and `last_place`, so the rail is remembered by the mechanism
    // that already remembers the pane you sized.
    let (dir, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-rail");
    app.save_last_place(&shell);
    let config = std::fs::read_to_string(dir.path().join("config.org")).expect("config.org");
    assert!(
        config.contains("rail_docked = true"),
        "the rail was not remembered:\n{config}"
    );

    // And re-read as the shell reads it at open.
    let cfg = closure_config::Config::from_path(&dir.path().join("config.org")).expect("parses");
    assert_eq!(cfg.rail_docked, Some(true));
}

#[test]
fn a_session_that_never_touched_it_writes_no_line() {
    // "a key appearing the first time you close the window, holding
    // the default, is noise in a file you read" — the rule the pane
    // width already follows.
    let (dir, shell, mut app) = app();
    app.save_last_place(&shell);
    let config = std::fs::read_to_string(dir.path().join("config.org")).unwrap_or_default();
    assert!(
        !config.contains("rail_docked"),
        "an untouched rail wrote itself into the config:\n{config}"
    );
}
