//! G6: palette + which-key polish. The command palette is fuzzy-ranked,
//! grouped into sections, and every entry carries a human description +
//! its chord — one hermetic source (`command_palette`) every GUI renders,
//! with a deterministic serialization (`serialize_palette`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_config::InputMode;
use closure_shell_core::{command_palette, serialize_palette};

#[test]
fn palette_groups_commands_into_sections_with_descriptions_and_chords() {
    let sections = command_palette("", InputMode::Notion);
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"Edit"), "has an Edit section: {titles:?}");
    assert!(
        titles.contains(&"Navigate"),
        "has a Navigate section: {titles:?}"
    );
    for s in &sections {
        assert!(!s.items.is_empty(), "no empty sections surface");
        for e in &s.items {
            assert!(!e.description.is_empty(), "{} has a description", e.label);
            assert!(!e.action.chord().is_empty(), "{} shows its chord", e.label);
        }
    }
}

#[test]
fn palette_fuzzy_filters_and_drops_empty_sections() {
    let sections = command_palette("rename", InputMode::Notion);
    assert_eq!(sections.len(), 1, "only the section with a match survives");
    assert_eq!(sections[0].title, "Edit");
    assert!(sections[0].items.iter().any(|e| e.label.contains("rename")));
}

#[test]
fn palette_serialises_deterministically() {
    let sections = command_palette("", InputMode::Notion);
    let a = serialize_palette(&sections);
    let b = serialize_palette(&sections);
    assert_eq!(a, b, "deterministic (I6)");
    assert!(a.contains("SECTION Edit"), "names sections: {a}");
    assert!(a.contains("Rename the headline"), "shows descriptions: {a}");
}

#[test]
fn palette_offers_fold_with_its_chord() {
    // The fold toggle is a palette command in every mode, carrying the
    // keymap chord (I4 — never hardcoded).
    for mode in [
        InputMode::Notion,
        InputMode::Emacs,
        InputMode::Vim,
        InputMode::Doom,
        InputMode::Helix,
    ] {
        let sections = command_palette("fold", mode);
        let entry = sections
            .iter()
            .flat_map(|s| &s.items)
            .find(|e| e.label == "fold")
            .unwrap_or_else(|| panic!("{mode:?} palette offers fold"));
        assert_eq!(
            entry.action.chord(),
            closure_input::chord_for_command(mode, "toggle-fold").unwrap(),
            "{mode:?} chord comes from the keymap"
        );
    }
}

// === The palette filters the way Doom's completion does ===
//
// Reported 2026-08-01: "when you try to filter for the add-sibling
// function, you have to type the -, in order to get a match. Just
// typing 'add sibling' won't match." Typing the exact punctuation of a
// command name is knowing the answer before you ask the question.

#[test]
fn typing_a_space_finds_a_hyphenated_command() {
    let sections = command_palette("add sibling", InputMode::Doom);
    let found: Vec<&str> = sections
        .iter()
        .flat_map(|s| &s.items)
        .map(|e| e.action.command())
        .collect();
    assert!(
        found.contains(&"add-sibling"),
        "the reported case: {found:?}"
    );
}

#[test]
fn the_components_can_come_in_any_order() {
    let sections = command_palette("sibling add", InputMode::Doom);
    let found: Vec<&str> = sections
        .iter()
        .flat_map(|s| &s.items)
        .map(|e| e.action.command())
        .collect();
    assert!(found.contains(&"add-sibling"), "{found:?}");
}

#[test]
fn a_component_that_matches_nothing_still_rules_the_entry_out() {
    let sections = command_palette("add zzzznope", InputMode::Doom);
    assert!(
        sections.is_empty(),
        "every component has to match, or the filter means nothing"
    );
}
