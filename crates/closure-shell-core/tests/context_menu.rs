//! Right-click context menus.
//!
//! The vision asks that every command show its keybinding in every UI
//! element it appears in. A context menu is the sharpest test of that:
//! it is the mouse-only path, and it is exactly where a user who does
//! not yet know the chords will discover them.
//!
//! So the menu is not a hand-written list in one shell's render
//! function. It is derived here, from the same keymap the chords
//! resolve against (I4), and every shell paints the same entries with
//! the same chords for the active mode.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ContextTarget, context_menu};

#[test]
fn the_row_menu_offers_the_structural_commands() {
    let menu = context_menu(ContextTarget::Row, InputMode::Doom);
    let commands: Vec<&str> = menu.iter().map(|i| i.action.command()).collect();
    for expected in [
        "rename",
        "toggle-todo",
        "cycle-priority",
        "edit-tags",
        "edit-body",
        "promote",
        "demote",
        "move-subtree-up",
        "move-subtree-down",
        "add-sibling",
        "backlinks",
        "delete",
    ] {
        assert!(
            commands.contains(&expected),
            "row menu is missing {expected}: {commands:?}"
        );
    }
}

#[test]
fn every_entry_carries_the_chord_that_runs_it() {
    // The whole point: no entry may appear without its binding, and
    // the binding must be the one the active mode actually resolves.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for target in [
            ContextTarget::Row,
            ContextTarget::Body,
            ContextTarget::Detail,
        ] {
            for item in context_menu(target, mode) {
                assert!(
                    !item.action.chord().is_empty(),
                    "{mode:?}/{target:?}: {} has no chord",
                    item.action.command()
                );
                assert_eq!(
                    closure_input::chord_for_command(mode, item.action.command()),
                    Some(item.action.chord()),
                    "{mode:?}: {} must show the keymap's chord",
                    item.action.command()
                );
                assert!(!item.label.is_empty(), "every entry needs a label");
            }
        }
    }
}

#[test]
fn the_body_menu_is_about_editing_not_structure() {
    let menu = context_menu(ContextTarget::Body, InputMode::Doom);
    let commands: Vec<&str> = menu.iter().map(|i| i.action.command()).collect();
    assert!(commands.contains(&"edit-body"), "{commands:?}");
    assert!(commands.contains(&"block-list"), "{commands:?}");
    assert!(
        !commands.contains(&"delete"),
        "deleting a subtree is not a body action: {commands:?}"
    );
}

#[test]
fn the_detail_menu_covers_the_fields_it_sits_on() {
    let menu = context_menu(ContextTarget::Detail, InputMode::Doom);
    let commands: Vec<&str> = menu.iter().map(|i| i.action.command()).collect();
    for expected in ["rename", "edit-tags", "edit-property", "toggle-todo"] {
        assert!(commands.contains(&expected), "{expected}: {commands:?}");
    }
}

#[test]
fn a_mode_without_a_binding_drops_the_entry_rather_than_lying() {
    // An entry with a blank chord would be a lie about the keymap; the
    // menu shrinks instead. Whatever survives is honest in every mode.
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Notion] {
        let menu = context_menu(ContextTarget::Row, mode);
        assert!(!menu.is_empty(), "{mode:?} must still offer something");
        assert!(
            menu.iter()
                .all(|i| closure_input::chord_for_command(mode, i.action.command()).is_some())
        );
    }
}

#[test]
fn menus_are_stable_across_calls() {
    // A menu that reorders itself under the pointer is unusable.
    let a = context_menu(ContextTarget::Row, InputMode::Doom);
    let b = context_menu(ContextTarget::Row, InputMode::Doom);
    let labels = |m: &[closure_shell_core::PaletteItemView]| {
        m.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    };
    assert_eq!(labels(&a), labels(&b));
}
