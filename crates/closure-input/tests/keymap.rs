//! The canonical per-mode keymap shared by every shell (TUI, gpui,
//! egui, web). Invariant: all five modes bind the *same command set*
//! (vision: every mode consistent in every UI); only the chords differ.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use closure_config::InputMode;
use closure_input::mode_keymap;

const MODES: [InputMode; 5] = [
    InputMode::Notion,
    InputMode::Emacs,
    InputMode::Vim,
    InputMode::Doom,
    InputMode::Helix,
];

fn commands(mode: InputMode) -> BTreeSet<&'static str> {
    mode_keymap(mode).iter().map(|(_, c)| *c).collect()
}

#[test]
fn every_mode_binds_the_same_command_set() {
    let reference = commands(InputMode::Doom);
    assert!(reference.contains("quit"));
    assert!(reference.contains("capture-start"));
    assert!(reference.contains("search-start"));
    for mode in MODES {
        assert_eq!(
            commands(mode),
            reference,
            "{mode:?} diverges from the command set"
        );
    }
}

#[test]
fn no_chord_is_bound_twice_within_a_mode() {
    for mode in MODES {
        let mut seen = BTreeSet::new();
        for (chord, _) in mode_keymap(mode) {
            assert!(seen.insert(*chord), "{mode:?} binds chord {chord:?} twice");
        }
    }
}

#[test]
fn modes_have_distinct_chords_for_navigation() {
    // Vim/Doom use j/k; Emacs uses C-n/C-p; Notion uses arrows only.
    let doom: BTreeSet<&str> = mode_keymap(InputMode::Doom)
        .iter()
        .map(|(c, _)| *c)
        .collect();
    assert!(doom.contains("j"));
    let emacs: BTreeSet<&str> = mode_keymap(InputMode::Emacs)
        .iter()
        .map(|(c, _)| *c)
        .collect();
    assert!(emacs.contains("C-n"));
    assert!(!emacs.contains("j"), "emacs must not bind bare j to nav");
    let notion: BTreeSet<&str> = mode_keymap(InputMode::Notion)
        .iter()
        .map(|(c, _)| *c)
        .collect();
    assert!(notion.contains("<down>"));
}

#[test]
fn command_for_resolves_a_full_chord() {
    // Convenience: resolve a chord string to its command in a mode.
    assert_eq!(
        closure_input::command_for(InputMode::Doom, "j"),
        Some("next-file")
    );
    assert_eq!(
        closure_input::command_for(InputMode::Emacs, "C-x C-c"),
        Some("quit")
    );
    assert_eq!(closure_input::command_for(InputMode::Doom, "no-such"), None);
}
