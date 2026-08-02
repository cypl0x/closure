//! "duplicate of undo-history in command palette."
//!
//! The Recent section exists to put the thing you just did at the top
//! before you have finished typing. It was *added* to the palette
//! rather than lifted out of it, so a command you had just run appeared
//! twice — once at the top and once in its own section, with the same
//! label and the same chord. A list that offers you the same command
//! twice is asking you to work out whether the two entries differ.
//!
//! They do not, so there is one of them: the promotion moves a command,
//! it does not copy it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::command_palette_with_history;

/// Every label the palette would show, in order, for `recent`.
fn labels(query: &str, recent: &[String]) -> Vec<String> {
    command_palette_with_history(query, InputMode::Doom, recent)
        .into_iter()
        .flat_map(|s| s.items)
        .map(|i| i.label)
        .collect()
}

/// The (section, label) pairs, for asserting *where* a command sits.
fn sections(query: &str, recent: &[String]) -> Vec<(String, String)> {
    command_palette_with_history(query, InputMode::Doom, recent)
        .into_iter()
        .flat_map(|s| {
            let title = s.title.clone();
            s.items.into_iter().map(move |i| (title.clone(), i.label))
        })
        .collect()
}

#[test]
fn a_recent_command_is_listed_once() {
    let recent = vec!["undo-history".to_owned()];
    let shown = labels("", &recent);
    let hits = shown.iter().filter(|l| *l == "undo-history").count();
    assert_eq!(hits, 1, "listed {hits} times: {shown:?}");
}

#[test]
fn the_one_listing_is_the_recent_one() {
    let recent = vec!["undo-history".to_owned()];
    let placed = sections("", &recent);
    let where_it_is: Vec<&String> = placed
        .iter()
        .filter(|(_, label)| label == "undo-history")
        .map(|(section, _)| section)
        .collect();
    assert_eq!(where_it_is, vec!["Recent"], "{placed:?}");
}

#[test]
fn a_curated_command_is_not_duplicated_either() {
    // `quit` has a curated label ("quit") and a section ("App"); being
    // recent must move it, not clone it.
    let recent = vec!["quit".to_owned()];
    let shown = labels("", &recent);
    assert_eq!(
        shown.iter().filter(|l| *l == "quit").count(),
        1,
        "{shown:?}"
    );
}

#[test]
fn promoting_the_only_member_of_a_section_drops_the_section() {
    // An empty section heading with nothing under it is furniture.
    let recent = vec!["quit".to_owned()];
    let titles: Vec<String> = command_palette_with_history("quit", InputMode::Doom, &recent)
        .into_iter()
        .map(|s| s.title)
        .collect();
    assert!(!titles.contains(&"App".to_owned()), "{titles:?}");
}

#[test]
fn nothing_recent_leaves_the_palette_as_it_was() {
    let plain = labels("", &[]);
    assert!(plain.contains(&"undo-history".to_owned()), "{plain:?}");
    assert!(!plain.is_empty());
}

#[test]
fn the_filter_still_reaches_a_recent_command() {
    // Promotion must not hide it from the query that would have found
    // it in its own section.
    let recent = vec!["undo-history".to_owned()];
    let shown = labels("undo hist", &recent);
    assert!(shown.contains(&"undo-history".to_owned()), "{shown:?}");
}
