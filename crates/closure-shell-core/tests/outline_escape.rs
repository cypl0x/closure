//! Walking a subtree out of its parent with the move chords.
//!
//! Reported 2026-08-01: "cannot move children (**) of headline (*) to
//! the top and promote to *". It was possible — `M-h` promotes and then
//! `M-k` moves — but the chord a hand reaches for first is `M-k`, and
//! on a first child that returned silently: no move, no message, no
//! hint that a second chord existed. A motion that does nothing and
//! says nothing reads as a broken feature.
//!
//! So the move chords walk *out* at the ends of a sibling run, the way
//! an outliner does: keep pressing `M-k` and the item rises through its
//! parents to the top of the file. `M-h` / `M-l` stay pure level
//! changes for when that is all you meant.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

// Real vaults carry `:ID:` drawers — closure writes one the first time
// it touches a headline — and the selection follows a moved subtree by
// id, so the fixture has to have them.
const SRC: &str = "* Parent\n:PROPERTIES:\n:ID: 01HQESC0000000000000001\n:END:\n\
                   ** Child\n:PROPERTIES:\n:ID: 01HQESC0000000000000002\n:END:\n\
                   *** Grand\n:PROPERTIES:\n:ID: 01HQESC0000000000000003\n:END:\n\
                   ** Sister\n:PROPERTIES:\n:ID: 01HQESC0000000000000004\n:END:\n\
                   * Next\n:PROPERTIES:\n:ID: 01HQESC0000000000000005\n:END:\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// `(title, level)` for every row, in outline order.
fn shape(app: &ModalApp, sh: &Shell) -> Vec<(String, u8)> {
    app.rows(sh)
        .into_iter()
        .map(|r| (r.title, r.level))
        .collect()
}

fn put_cursor_on(app: &mut ModalApp, sh: &Shell, title: &str) {
    let at = app
        .rows(sh)
        .iter()
        .position(|r| r.title == title)
        .unwrap_or_else(|| panic!("no row {title}"));
    app.select(at, sh);
}

#[test]
fn moving_a_first_child_up_lifts_it_out_above_its_parent() {
    let (_d, mut sh, mut app) = fixture();
    put_cursor_on(&mut app, &sh, "Child");
    app.run(&mut sh, "move-subtree-up");
    assert_eq!(
        shape(&app, &sh),
        [
            ("Child".to_owned(), 1),
            ("Grand".to_owned(), 2),
            ("Parent".to_owned(), 1),
            ("Sister".to_owned(), 2),
            ("Next".to_owned(), 1),
        ],
        "out of Parent, above it, at Parent's level"
    );
}

#[test]
fn the_subtree_comes_with_it() {
    let (_d, mut sh, mut app) = fixture();
    put_cursor_on(&mut app, &sh, "Child");
    app.run(&mut sh, "move-subtree-up");
    let rows = shape(&app, &sh);
    let child = rows.iter().position(|(t, _)| t == "Child").expect("Child");
    let grand = rows.iter().position(|(t, _)| t == "Grand").expect("Grand");
    assert_eq!(grand, child + 1, "Grand stayed under Child");
    assert_eq!(rows[grand].1, 2, "one level under it, as before");
}

#[test]
fn the_cursor_follows_the_headline_that_moved() {
    let (_d, mut sh, mut app) = fixture();
    put_cursor_on(&mut app, &sh, "Child");
    app.run(&mut sh, "move-subtree-up");
    assert_eq!(
        app.detail(&sh).expect("detail").title,
        "Child",
        "org's rule: the selection rides the move"
    );
}

#[test]
fn repeating_it_walks_all_the_way_to_the_top() {
    // The whole of the report: "move children to the top and promote".
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* A\n:PROPERTIES:\n:ID: 01HQESC0000000000000011\n:END:\n** B\n:PROPERTIES:\n:ID: 01HQESC0000000000000012\n:END:\n*** Deep\n:PROPERTIES:\n:ID: 01HQESC0000000000000013\n:END:\n* Z\n:PROPERTIES:\n:ID: 01HQESC0000000000000014\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (mut sh, mut app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    put_cursor_on(&mut app, &sh, "Deep");
    for _ in 0..4 {
        app.run(&mut sh, "move-subtree-up");
    }
    assert_eq!(
        shape(&app, &sh).first(),
        Some(&("Deep".to_owned(), 1)),
        "top of the file, at level 1: {:?}",
        shape(&app, &sh)
    );
}

#[test]
fn a_middle_child_still_just_swaps_with_its_sibling() {
    // Escaping is what happens at the *ends* of a sibling run. In the
    // middle it is still an ordinary reorder, and must not change level.
    let (_d, mut sh, mut app) = fixture();
    put_cursor_on(&mut app, &sh, "Sister");
    app.run(&mut sh, "move-subtree-up");
    assert_eq!(
        shape(&app, &sh),
        [
            ("Parent".to_owned(), 1),
            ("Sister".to_owned(), 2),
            ("Child".to_owned(), 2),
            ("Grand".to_owned(), 3),
            ("Next".to_owned(), 1),
        ],
        "swapped with Child, still a child of Parent"
    );
}

#[test]
fn moving_a_last_child_down_lifts_it_out_below_its_parent() {
    let (_d, mut sh, mut app) = fixture();
    put_cursor_on(&mut app, &sh, "Sister");
    app.run(&mut sh, "move-subtree-down");
    assert_eq!(
        shape(&app, &sh),
        [
            ("Parent".to_owned(), 1),
            ("Child".to_owned(), 2),
            ("Grand".to_owned(), 3),
            ("Sister".to_owned(), 1),
            ("Next".to_owned(), 1),
        ],
        "out of Parent, after it, at Parent's level"
    );
}

#[test]
fn a_top_level_headline_with_nowhere_to_go_says_so() {
    // There is no level to escape to, so the motion has to explain
    // itself rather than return in silence — which is what made the
    // original case read as broken.
    let (_d, mut sh, mut app) = fixture();
    put_cursor_on(&mut app, &sh, "Parent");
    let before = shape(&app, &sh);
    app.run(&mut sh, "move-subtree-up");
    assert_eq!(shape(&app, &sh), before, "nothing moved");
    assert!(
        !app.status().is_empty(),
        "and it said why: {:?}",
        app.status()
    );
}
