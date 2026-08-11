//! "Every mode should be consistently implemented in every ui
//! element/widget/input."
//!
//! Notion mode is meant to be the default and the one a newcomer
//! meets, and it is also the one with the fewest chords — mouse and
//! slash commands instead. Those two facts pull against each other: a
//! mode with few chords is a mode where a command can quietly become
//! unreachable, and nothing said so.
//!
//! What parity can honestly mean here is not "the same chords". The
//! modes differ on purpose — that is what choosing one is for. It is
//! that no mode is *missing* something the others treat as
//! fundamental, and that anything reachable at all is reachable in
//! every mode, because a command bound in four modes and not the fifth
//! is a bug rather than a design.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_input::mode_keymap;

const MODES: [InputMode; 5] = [
    InputMode::Doom,
    InputMode::Vim,
    InputMode::Emacs,
    InputMode::Helix,
    InputMode::Notion,
];

/// Every command reachable by a chord in `mode`.
fn commands(mode: InputMode) -> std::collections::BTreeSet<&'static str> {
    mode_keymap(mode).iter().map(|(_, cmd)| *cmd).collect()
}

#[test]
fn every_mode_reaches_the_commands_a_notebook_cannot_do_without() {
    // The floor, spelled out rather than left to the union: these are
    // the things a person opening closure in any mode has to be able
    // to do, and a mode missing one is broken however elegant its
    // remaining chords are.
    for mode in MODES {
        let have = commands(mode);
        for needed in [
            "palette",
            "capture",
            "save-buffer",
            "undo",
            "search",
            "manual",
            "describe-key",
            "describe-command",
        ] {
            assert!(have.contains(needed), "{mode:?} cannot reach `{needed}`");
        }
    }
}

#[test]
fn a_command_bound_in_every_other_mode_is_bound_here_too() {
    // The rule that catches the real failure: a command added to four
    // keymaps and forgotten in the fifth. Not "the same chord" — the
    // modes differ on purpose — but "reachable at all".
    for mode in MODES {
        let mine = commands(mode);
        let others: Vec<std::collections::BTreeSet<&str>> = MODES
            .iter()
            .filter(|m| **m != mode)
            .map(|m| commands(*m))
            .collect();
        let in_all_others: Vec<&str> = others[0]
            .iter()
            .filter(|c| others[1..].iter().all(|o| o.contains(*c)))
            .copied()
            .collect();
        let missing: Vec<&str> = in_all_others
            .into_iter()
            .filter(|c| !mine.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "{mode:?} is the only mode that cannot reach: {missing:?}"
        );
    }
}

#[test]
fn notion_mode_is_not_a_stub() {
    // It is meant to be the default. A mode with a handful of chords
    // and a mouse is a design; a mode with three is an unfinished one.
    let have = commands(InputMode::Notion);
    assert!(have.len() >= 20, "only {} commands: {have:?}", have.len());
}

// The fourth rule — no mode binds a name nothing implements — is not
// here, and could not be: `closure-input` sits below the crate that
// owns the registry, and reaching up for it would invert the layering
// this workspace is built on. It is enforced from the other side by
// `closure-tui/tests/no_dead_chords.rs`, which walks every bound
// command through a shell and fails on the ones it cannot serve.
