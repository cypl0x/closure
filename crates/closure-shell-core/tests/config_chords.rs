//! "Are all of the keybindings configureable through the config.org?
//! Or are the chords hardcoded in Rust? Event the Emacs readline ones?"
//!
//! They were hardcoded. `config.org` could choose *which* of the five
//! keymaps you got and nothing inside it, so a chord you disliked was a
//! chord you kept, and the answer to "can I move this key" was "send a
//! patch".
//!
//! `bind <chord> = <command>` in the `closure-config` block rebinds one
//! chord; `bind <chord> =` with nothing after it takes the chord away.
//! They apply on top of the mode you chose, in file order, so the
//! keymap you get is the one you started from plus what you said about
//! it — and every lookup in the shell reads that one, so a rebound
//! chord moves in the which-key panel, the palette and the tutorial at
//! the same time as it moves under your fingers.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::{Config, InputMode};
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

/// A vault whose `config.org` carries `binds`.
fn fixture(binds: &str) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQBIND000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    fs::write(
        dir.path().join("config.org"),
        format!(
            "#+TITLE: t\n\n#+BEGIN_SRC closure-config\ninput_mode = doom\n{binds}\n#+END_SRC\n"
        ),
    )
    .expect("write");
    let cfg = Config::from_path(&dir.path().join("config.org")).expect("config parses");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(cfg.input_mode);
    app.set_key_overrides(cfg.key_bindings);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQBIND000000000000001"));
    (dir, shell, app)
}

#[test]
fn a_bind_line_parses_into_a_chord_and_a_command() {
    let cfg = Config::from_kv_block("bind g z = toggle-wrap\n").expect("parses");
    assert_eq!(
        cfg.key_bindings,
        vec![("g z".to_owned(), "toggle-wrap".to_owned())]
    );
}

#[test]
fn a_bind_with_nothing_after_it_is_an_unbind() {
    let cfg = Config::from_kv_block("bind g W =\n").expect("parses");
    assert_eq!(cfg.key_bindings, vec![("g W".to_owned(), String::new())]);
}

#[test]
fn a_bind_with_no_chord_is_an_error_rather_than_a_no_op() {
    // Silently ignoring a line the user wrote is how a config file
    // becomes something you cannot trust.
    assert!(Config::from_kv_block("bind = toggle-wrap\n").is_err());
}

#[test]
fn a_rebound_chord_runs_the_command_it_was_given() {
    let (_d, mut shell, mut app) = fixture("bind g z = toggle-wrap");
    let before = app.wrap();
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "z", false, false, Some('z'));
    assert_ne!(app.wrap(), before, "g z toggled wrap: {}", app.status());
}

#[test]
fn the_chord_it_replaced_still_works_unless_you_took_it_away() {
    // Adding a key is not moving a key: `g W` keeps working until the
    // config says otherwise.
    let (_d, mut shell, mut app) = fixture("bind g z = toggle-wrap");
    let before = app.wrap();
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "W", false, false, Some('W'));
    assert_ne!(app.wrap(), before);
}

#[test]
fn an_unbind_takes_the_chord_away() {
    let (_d, mut shell, mut app) = fixture("bind g W =");
    let before = app.wrap();
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "W", false, false, Some('W'));
    assert_eq!(app.wrap(), before, "g W does nothing now: {}", app.status());
    assert!(app.chords_for("toggle-wrap").iter().all(|c| *c != "g W"));
}

#[test]
fn rebinding_a_chord_that_was_taken_moves_it() {
    // Two lines, the ordinary way to move a key: take it off the old
    // command, put it on the new one.
    let (_d, mut shell, mut app) = fixture("bind g m = messages");
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "m", false, false, Some('m'));
    assert_eq!(app.surface(), ModalSurface::Messages, "{}", app.status());
}

#[test]
fn the_which_key_panel_shows_the_chord_you_chose() {
    // A rebound chord that the panel still lists under its old key is a
    // panel that lies, which is worse than a panel that is empty.
    let (_d, _sh, app) = fixture("bind g z = toggle-wrap");
    let listed: Vec<String> = app
        .which_key_groups()
        .into_iter()
        .flat_map(|(_, entries)| entries)
        .filter(|(_, cmd)| cmd == "toggle-wrap")
        .map(|(chord, _)| chord)
        .collect();
    assert!(listed.contains(&"g z".to_owned()), "{listed:?}");
}

#[test]
fn the_palette_shows_it_too() {
    let (_d, mut shell, mut app) = fixture("bind g z = toggle-wrap");
    app.run(&mut shell, "palette");
    for c in "toggle-wrap".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let entry = app
        .palette_entries()
        .into_iter()
        .find(|e| e.action.command() == "toggle-wrap")
        .expect("in the palette");
    assert!(
        entry.action.chords().iter().any(|c| c == "g z"),
        "{:?}",
        entry.action.chords()
    );
}

#[test]
fn an_override_naming_a_command_that_does_not_exist_is_reported() {
    // It cannot be caught at parse time — the config crate does not know
    // the command list — so the shell says so rather than binding a key
    // to nothing and leaving you to find out by pressing it.
    let (_d, _sh, app) = fixture("bind g z = not-a-command");
    assert!(
        app.messages().iter().any(|m| m.contains("not-a-command")),
        "{:?}",
        app.messages()
    );
}

#[test]
fn a_vault_with_no_binds_gets_the_mode_it_asked_for() {
    let (_d, _sh, app) = fixture("");
    assert_eq!(app.chord_for("toggle-wrap"), Some("g W"));
    assert_eq!(
        app.keymap().len(),
        closure_input::mode_keymap(InputMode::Doom).len(),
        "the same keymap, not a copy that drifted"
    );
}
