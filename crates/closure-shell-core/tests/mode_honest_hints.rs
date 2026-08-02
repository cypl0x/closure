//! A hint that names a chord the current mode does not have is a lie.
//!
//! Reported 2026-08-02: "vim :q! and other bindings are shown in the
//! editor even if I am in Notion or Emacs mode. Only and everywhere
//! only show the keybindings that are relevant to the corresponding
//! mode. Hide irrelevant ones."
//!
//! The editor's vocabulary line was written for a modal buffer and
//! shown in every mode. Notion and Emacs have no NORMAL to drop into —
//! `entry_mode` puts their buffers straight into INSERT and `Esc`
//! closes them — so "Esc → NORMAL" was an instruction that does
//! something else entirely.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{EditorMode, editor_hint};

#[test]
fn a_modal_mode_is_told_about_normal() {
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Helix] {
        let hint = editor_hint(EditorMode::Insert, mode);
        assert!(hint.contains("NORMAL"), "{mode:?}: {hint}");
    }
}

#[test]
fn a_mode_without_normal_is_not() {
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let hint = editor_hint(EditorMode::Insert, mode);
        assert!(
            !hint.contains("NORMAL"),
            "{mode:?} has no NORMAL to send you to: {hint}"
        );
    }
}

#[test]
fn the_readline_chords_are_advertised_everywhere() {
    // These *are* in every mode — the whole point of the parity work —
    // so hiding them would be the opposite mistake.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let hint = editor_hint(EditorMode::Insert, mode);
        assert!(hint.contains("readline"), "{mode:?}: {hint}");
    }
}

#[test]
fn the_modal_vocabulary_is_only_offered_to_modal_modes() {
    // NORMAL is unreachable in Notion and Emacs, so its vocabulary
    // cannot appear there whatever is asked for.
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let hint = editor_hint(EditorMode::Normal, mode);
        assert!(!hint.contains("dd yy"), "{mode:?}: {hint}");
    }
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Helix] {
        assert!(editor_hint(EditorMode::Normal, mode).contains("dd yy"));
    }
}

#[test]
fn a_hint_never_names_a_chord_the_mode_cannot_run() {
    // The general rule behind the report, checked over the whole set:
    // every `:x`-style ex command named in a hint has to be reachable,
    // and `:` is bound in all five modes, so these are honest — but the
    // check is here so a future hint cannot quietly stop being.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for editor in [EditorMode::Insert, EditorMode::Normal, EditorMode::Visual] {
            let hint = editor_hint(editor, mode);
            if hint.contains(":q") || hint.contains(":w") {
                assert!(
                    closure_input::command_for(mode, ":") == Some("ex-command"),
                    "{mode:?} names an ex command it cannot open"
                );
            }
        }
    }
}
