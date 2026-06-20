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
fn every_mode_binds_toggle_llm_render() {
    // V3b: the live render-permission toggle is reachable (with a chord)
    // in every mode's which-key.
    for mode in MODES {
        assert!(
            closure_input::chord_for_command(mode, "toggle-llm-render").is_some(),
            "{mode:?} binds toggle-llm-render"
        );
    }
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

#[test]
fn launcher_edit_commands_bound_in_every_mode() {
    // G4: the launcher's edit actions are part of the canonical command
    // set (I4) so every shell can show their real chord, not a hardcoded
    // string. All five modes must bind them (chords differ).
    for mode in MODES {
        let set = commands(mode);
        for cmd in ["add-sibling", "rename", "delete"] {
            assert!(set.contains(cmd), "{mode:?} missing {cmd}");
        }
    }
}

#[test]
fn chord_for_command_is_the_reverse_of_command_for() {
    use closure_input::{chord_for_command, command_for};
    for mode in MODES {
        for (_chord, cmd) in mode_keymap(mode) {
            // The first chord bound to a command round-trips.
            let got = chord_for_command(mode, cmd).expect("command has a chord");
            assert_eq!(
                command_for(mode, got),
                Some(*cmd),
                "{mode:?}: {cmd} -> {got} -> command"
            );
        }
        assert_eq!(chord_for_command(mode, "no-such-command"), None);
    }
    // Spot-check a concrete pair.
    assert_eq!(chord_for_command(InputMode::Vim, "next-file"), Some("j"));
}
