//! "multiple chords/keybindings for a single command/function"
//!
//! The keymaps have carried several chords for one command since they
//! were written — 45 commands in Doom alone, `C-s` and `SPC f s` both
//! saving, `SPC q q` and `C-q` and `ZZ` all quitting. What was missing
//! is that anything could *see* it: the only reverse lookup in the
//! crate was `chord_for_command`, singular, and everything that shows
//! a command next to its key — the palette, the tutorial, every button
//! — went through it. So the second chord worked and nothing told you
//! it existed, which for a keybinding is most of the value.
//!
//! One command, every chord that runs it, everywhere a command is
//! named.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell, tutorial_org};
use closure_store::Vault;
use tempfile::TempDir;

const MODES: [InputMode; 5] = [
    InputMode::Doom,
    InputMode::Vim,
    InputMode::Emacs,
    InputMode::Helix,
    InputMode::Notion,
];

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQCHORD00000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    app.select_by_id(&shell, "01HQCHORD00000000000001");
    (dir, shell, app)
}

#[test]
fn a_command_can_name_every_chord_that_runs_it() {
    let all = closure_input::chords_for_command(InputMode::Doom, "save-buffer");
    assert!(all.len() > 1, "Doom saves with more than one key: {all:?}");
    assert!(all.contains(&"C-s"), "{all:?}");
    assert!(all.contains(&"SPC f s"), "{all:?}");
}

#[test]
fn the_first_one_is_the_one_a_single_slot_gets() {
    // Where there is room for one chord, it is the first the keymap
    // lists — so the two lookups never disagree about which is primary.
    for mode in MODES {
        for cmd in ["quit", "capture", "palette", "save-buffer"] {
            let all = closure_input::chords_for_command(mode, cmd);
            assert_eq!(
                closure_input::chord_for_command(mode, cmd),
                all.first().copied(),
                "{mode:?} {cmd}"
            );
        }
    }
}

#[test]
fn an_unbound_command_has_no_chords_rather_than_one_empty_one() {
    assert!(closure_input::chords_for_command(InputMode::Vim, "not-a-command").is_empty());
}

#[test]
fn every_listed_chord_actually_runs_the_command() {
    // The list is only worth showing if pressing any of them works.
    for mode in MODES {
        for (chord, cmd) in closure_input::mode_keymap(mode) {
            let all = closure_input::chords_for_command(mode, cmd);
            assert!(
                all.contains(chord),
                "{mode:?}: {chord} runs {cmd} but is not listed for it"
            );
            assert_eq!(
                closure_input::command_for(mode, chord),
                Some(*cmd),
                "{mode:?}: {chord} listed for {cmd} but dispatches elsewhere"
            );
        }
    }
}

#[test]
fn the_app_hands_the_whole_list_to_whatever_is_painting() {
    let (_d, _sh, app) = fixture();
    let all = app.chords_for("save-buffer");
    assert!(all.len() > 1, "{all:?}");
    assert_eq!(app.chord_for("save-buffer"), all.first().copied());
}

#[test]
fn the_palette_shows_the_alternates() {
    // The palette is where you go when you do not know the key, so it
    // is the place a second key is worth knowing about.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    for c in "save-buffer".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let entry = app
        .palette_entries()
        .into_iter()
        .find(|e| e.action.command() == "save-buffer")
        .expect("save-buffer in the palette");
    let shown = entry.action.chords();
    assert!(shown.len() > 1, "one chord shown of several: {shown:?}");
    assert_eq!(
        shown.first().map(String::as_str),
        Some(entry.action.chord())
    );
}

#[test]
fn the_tutorial_names_them_all() {
    // A tutorial that teaches one of two keys teaches half a keymap,
    // and the half it drops is the one you would have preferred.
    let org = tutorial_org(InputMode::Doom);
    assert!(org.contains("C-s"), "the readline key: {}", &org[..0]);
    assert!(org.contains("SPC f s"), "and the leader one");
}

#[test]
fn a_command_bound_once_still_reads_as_one_chord() {
    // The common case must not grow a separator with nothing after it.
    let (_d, _sh, app) = fixture();
    let one = closure_input::chords_for_command(InputMode::Doom, "backlinks");
    assert_eq!(one.len(), 1, "{one:?}");
    assert_eq!(app.chords_for("backlinks"), one);
}
