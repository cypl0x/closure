//! "command palette issues"
//!
//! Filed with an empty body, so: the palette, looked at against
//! everything else this session has settled.
//!
//! Eighty-nine commands are bound to a chord and nineteen of them had
//! a description. The other seventy-two fell back to `"Run <name>"` —
//! a description column that repeats the label and tells you nothing
//! you could not already read, on 81% of the rows. A palette is the
//! place you go when you do *not* know what a command is called, so a
//! column that answers "what does this do?" with the name again is the
//! one thing it must not do.
//!
//! Every bound command says what it does now, and this test is what
//! keeps that true when the next one is added.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;

/// Every command any keymap binds a chord to.
fn bound_commands() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ]
    .into_iter()
    .flat_map(|m| closure_input::mode_keymap(m).iter().map(|(_, c)| *c))
    .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn every_bound_command_says_what_it_does() {
    let described: std::collections::BTreeSet<&str> = closure_shell_core::palette_command_names()
        .into_iter()
        .collect();
    let missing: Vec<&str> = bound_commands()
        .into_iter()
        .filter(|c| !described.contains(c))
        .collect();
    assert!(
        missing.is_empty(),
        "{} command(s) fall back to \"Run <name>\": {missing:?}",
        missing.len()
    );
}

#[test]
fn no_description_merely_repeats_the_name() {
    // The fallback's exact shape, in case one is ever pasted in by
    // hand: "Run next-buffer" is not a description. The rule is that
    // exact string and not "starts with Run" — "Run the source block
    // and keep its output" is a perfectly good sentence, and a first
    // version of this test rejected it.
    for (label, canonical, _, desc) in closure_shell_core::palette_entries_raw() {
        assert_ne!(desc, format!("Run {canonical}"), "{label}: the placeholder");
        assert!(
            desc.to_lowercase() != label.to_lowercase(),
            "{label}: the description is the label"
        );
        assert!(desc.len() > label.len(), "{label}: {desc:?} says no more");
    }
}

#[test]
fn every_description_reads_as_a_sentence_fragment() {
    // A palette column is scanned, not read: they all start with a
    // capital and none ends in a full stop.
    for (label, desc) in closure_shell_core::palette_descriptions() {
        let first = desc.chars().next().expect("not empty");
        assert!(first.is_uppercase(), "{label}: {desc:?}");
        assert!(!desc.ends_with('.'), "{label}: {desc:?}");
    }
}

#[test]
fn every_command_sits_in_a_real_section() {
    let sections = closure_shell_core::palette_section_names();
    for (label, section) in closure_shell_core::palette_sections_of() {
        assert!(
            sections.contains(&section),
            "{label} is in {section:?}, which is not a section"
        );
    }
}

#[test]
fn a_command_is_found_by_its_name_as_well_as_its_label() {
    // Giving the commands readable labels cost this, and a test caught
    // it: "toggle wrap" reads better in the list, and somebody who
    // knows the command types `toggle-wrap`. Both have to find it —
    // muscle memory is one reason and an LLM calling a command by name
    // is the other.
    for (_, canonical, _, _) in closure_shell_core::palette_entries_raw() {
        let found = closure_shell_core::command_palette(canonical, InputMode::Doom)
            .into_iter()
            .flat_map(|s| s.items)
            .any(|e| e.action.command() == canonical);
        assert!(found, "typing `{canonical}` does not find it");
    }
}
