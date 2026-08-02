//! "which-key content may have to be rearranged. Especially the initial
//! SPC or ? aligns the keybinds in a way that you have to scroll in
//! which-key. In Doom Emacs which-key keybindings get distributed like
//! you can see in the screenshot. In closure you have to scroll to read
//! the bottom most element."
//!
//! closure gave each group a column of its own. Six groups, six
//! columns — and "Command" holds more bindings than the other five put
//! together, so its column ran off the bottom of the window while the
//! five beside it stood half empty. Doom flows one flat list into
//! balanced columns instead, which is why nothing there scrolls.
//!
//! Newspaper columns, then: entries fill a column to the height the
//! panel has and continue in the next. The group headings survive —
//! they are what makes the panel skimmable, and Doom's has none —
//! which costs one rule: a heading may not be the last cell of a
//! column, because a heading with its group in the next column is
//! worse than no heading.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{WhichKeyCell, which_key_columns};

/// Six groups shaped like the real ones: five short, one long.
fn groups() -> Vec<(String, Vec<(String, String)>)> {
    let entry = |i: usize| (format!("k{i}"), format!("cmd{i}"));
    vec![
        ("Navigate".to_owned(), (0..6).map(entry).collect()),
        ("Edit".to_owned(), (10..16).map(entry).collect()),
        ("View".to_owned(), (20..23).map(entry).collect()),
        ("Mode".to_owned(), (30..32).map(entry).collect()),
        ("App".to_owned(), (40..46).map(entry).collect()),
        ("Command".to_owned(), (50..90).map(entry).collect()),
    ]
}

/// Every entry in `cols`, in reading order.
fn entries(cols: &[Vec<WhichKeyCell>]) -> Vec<String> {
    cols.iter()
        .flatten()
        .filter_map(|c| match c {
            WhichKeyCell::Entry { chord, .. } => Some(chord.clone()),
            WhichKeyCell::Heading(_) => None,
        })
        .collect()
}

#[test]
fn no_column_is_taller_than_the_panel() {
    // The bug: one column was as tall as its group, however long that
    // was, and the panel could not show it.
    for height in [8usize, 12, 20, 40] {
        for col in which_key_columns(&groups(), height) {
            assert!(col.len() <= height, "a column of {} at {height}", col.len());
        }
    }
}

#[test]
fn every_binding_is_shown_exactly_once() {
    // Reflowing must not drop or duplicate a chord.
    let cols = which_key_columns(&groups(), 12);
    let shown = entries(&cols);
    let total: usize = groups().iter().map(|(_, e)| e.len()).sum();
    assert_eq!(shown.len(), total, "{} of {total}", shown.len());
    let mut sorted = shown.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), shown.len(), "a chord shown twice");
}

#[test]
fn the_order_survives_the_reflow() {
    // Column-major: read a column down, then the next. A binding must
    // not move relative to its neighbours.
    let cols = which_key_columns(&groups(), 12);
    let shown = entries(&cols);
    let flat: Vec<String> = groups()
        .into_iter()
        .flat_map(|(_, e)| e.into_iter().map(|(c, _)| c))
        .collect();
    assert_eq!(shown, flat);
}

#[test]
fn a_heading_is_never_the_last_cell_of_a_column() {
    // A heading with its group in the next column is worse than no
    // heading at all.
    for height in [6usize, 8, 12, 20] {
        for col in which_key_columns(&groups(), height) {
            assert!(
                !matches!(col.last(), Some(WhichKeyCell::Heading(_))),
                "orphan heading at height {height}"
            );
        }
    }
}

#[test]
fn every_group_still_says_its_name() {
    let cols = which_key_columns(&groups(), 12);
    let headings: Vec<String> = cols
        .iter()
        .flatten()
        .filter_map(|c| match c {
            WhichKeyCell::Heading(t) => Some(t.clone()),
            WhichKeyCell::Entry { .. } => None,
        })
        .collect();
    for (name, _) in groups() {
        assert!(headings.contains(&name), "{name} lost its heading");
    }
}

#[test]
fn a_long_group_is_split_across_columns_rather_than_cut_off() {
    // "Command" is longer than any panel is tall, so it has to
    // continue rather than scroll.
    let cols = which_key_columns(&groups(), 10);
    assert!(
        cols.len() > 6,
        "only {} columns for 63 bindings",
        cols.len()
    );
}

#[test]
fn nothing_at_all_produces_nothing() {
    assert!(which_key_columns(&[], 10).is_empty());
}

#[test]
fn a_height_of_zero_does_not_loop_forever() {
    // A panel too short to show anything is a panel, not a hang.
    let cols = which_key_columns(&groups(), 0);
    assert!(cols.is_empty() || cols.iter().all(|c| c.len() <= 1));
}
