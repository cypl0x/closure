//! The tutorial closure writes into your vault.
//!
//! A hand-written tutorial is wrong the first time a chord moves, and
//! nobody notices until a new user follows it and it does not work. So
//! the chords in it come from the same keymap the app dispatches
//! through (I4) — if `SPC f s` stops saving, this file says so the next
//! time it is written, and the test below fails first.
//!
//! It is an org file in the vault, because that is what closure is: the
//! tutorial is a note you can fold, search, edit and sync like any
//! other.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::tutorial_org;

#[test]
fn it_is_a_valid_org_document() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let text = tutorial_org(mode);
        closure_org::parse(&text).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        assert!(text.starts_with("#+TITLE:"), "{mode:?}");
    }
}

#[test]
fn the_chords_are_the_ones_the_app_actually_dispatches() {
    // The whole point: no chord in the tutorial is typed by hand.
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Notion] {
        let text = tutorial_org(mode);
        for command in ["capture-start", "palette", "search-start", "quit"] {
            let chord =
                closure_input::chord_for_command(mode, command).expect("bound in every mode");
            assert!(
                text.contains(chord),
                "{mode:?}: the tutorial never mentions `{chord}` for {command}"
            );
        }
    }
}

#[test]
fn it_covers_the_things_a_new_user_has_to_be_told() {
    let text = tutorial_org(InputMode::Doom);
    for topic in [
        "capture",
        "config.org",
        "Pairing",
        "eval_trust",
        "undo",
        "tag",
    ] {
        assert!(text.contains(topic), "no section mentions {topic}");
    }
}

#[test]
fn it_explains_the_registers_and_the_editor_chords() {
    // Asked for by name: "implement vim registers and put a
    // tutorial/explanation of these into the vault".
    let text = tutorial_org(InputMode::Doom);
    assert!(text.contains("register"), "registers are explained");
    assert!(text.contains("\"a"), "and named ones are shown");
    assert!(text.contains(":w"), "and how a buffer is written");
}

#[test]
fn every_mode_gets_its_own_spelling() {
    // A Doom user and a Notion user are told different things, because
    // the app *is* different for them.
    let doom = tutorial_org(InputMode::Doom);
    let notion = tutorial_org(InputMode::Notion);
    assert_ne!(doom, notion);
    assert!(doom.contains("SPC"), "Doom's leader is named");
    assert!(
        notion.contains("INSERT") || notion.contains("types"),
        "Notion is told it has no modes to worry about"
    );
}
