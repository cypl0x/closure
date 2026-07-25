//! What a frame is allowed to cost.
//!
//! Scrolling repaints. Every repaint re-derives whatever the panes
//! read, so anything expensive on that path is paid once per frame at
//! wheel speed — which is exactly how the outline and the palette
//! ended up feeling slow despite the row list already being memoised.
//!
//! Three things were doing real work per frame:
//!
//!  * the outline asked the vault whether each visible row was folded,
//!    one index lookup per row per frame, when the fold state was
//!    already known while the rows were derived;
//!  * the detail pane re-cloned the selected headline (body, tags,
//!    properties) and re-highlighted the whole body, even though
//!    scrolling does not change the selection;
//!  * the palette rebuilt every entry — fuzzy score, keymap scan for
//!    the chord, a `String` per field — on every frame *and* on every
//!    keystroke, twice.
//!
//! These tests pin the derivations as memoised and exact. They are
//! about allocation counts, not wall-clock, so they are hermetic and
//! do not flake on a loaded machine.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    let mut src = String::new();
    for i in 0..40 {
        src.push_str(&format!(
            "* TODO Head {i}\n:PROPERTIES:\n:ID: 01HQBUDGET{i:014}\n:END:\nbody line for {i}\n"
        ));
    }
    fs::write(dir.path().join("notes.org"), src).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

// === the fold flag rides along with the row ===

#[test]
fn rows_carry_their_own_fold_state() {
    // The outline needs it for every visible row on every frame, and
    // `derive_rows` already computed it to decide what to hide.
    let (_d, mut shell, mut app) = fixture();
    assert!(
        app.rows(&shell).iter().all(|r| !r.folded),
        "nothing folded yet"
    );
    app.select(0, &shell);
    app.run(&mut shell, "toggle-fold");
    let rows = app.rows(&shell);
    assert!(rows[0].folded, "the folded row says so: {:?}", rows[0]);
    assert!(rows[1..].iter().all(|r| !r.folded), "and only that one");
}

#[test]
fn the_fold_flag_agrees_with_the_vault() {
    let (_d, mut shell, mut app) = fixture();
    app.select(3, &shell);
    app.run(&mut shell, "toggle-fold");
    for row in app.rows(&shell).iter() {
        assert_eq!(
            row.folded,
            closure_shell_core::is_row_folded(&shell, &row.id),
            "row {} disagrees with the vault",
            row.title
        );
    }
}

// === the detail pane ===

#[test]
fn the_detail_is_derived_once_per_selection() {
    let (_d, shell, app) = fixture();
    let before = app.detail_recomputes();
    for _ in 0..30 {
        assert!(app.detail(&shell).is_some());
    }
    assert_eq!(
        app.detail_recomputes() - before,
        1,
        "30 repaints of an unchanged selection is one derivation"
    );
}

#[test]
fn moving_the_selection_re_derives_the_detail() {
    let (_d, shell, mut app) = fixture();
    let first = app.detail(&shell).expect("detail").title;
    let after_warm = app.detail_recomputes();
    app.select(5, &shell);
    let second = app.detail(&shell).expect("detail").title;
    assert_ne!(first, second, "different row, different detail");
    assert!(app.detail_recomputes() > after_warm);
}

#[test]
fn editing_the_selected_row_re_derives_the_detail() {
    // The memo must not outlive the content it described.
    let (_d, mut shell, app) = fixture();
    assert_eq!(app.detail(&shell).expect("detail").title, "Head 0");
    shell
        .rename_headline(
            &closure_core::BlockId::from_existing("01HQBUDGET00000000000000"),
            "Renamed",
        )
        .expect("rename");
    assert_eq!(
        app.detail(&shell).expect("detail").title,
        "Renamed",
        "a stale detail would still say Head 0"
    );
}

// === the palette ===

#[test]
fn palette_entries_are_derived_once_per_query() {
    let (_d, _shell, app) = fixture();
    let before = app.palette_recomputes();
    for _ in 0..30 {
        assert!(!app.palette_entries().is_empty());
    }
    assert_eq!(
        app.palette_recomputes() - before,
        1,
        "repainting the palette must not rebuild it"
    );
}

#[test]
fn typing_in_the_palette_re_derives_it() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    let all = app.palette_entries().len();
    let after_warm = app.palette_recomputes();
    app.on_key(&mut shell, "r", false, false, Some('r'));
    let filtered = app.palette_entries();
    assert!(app.palette_recomputes() > after_warm, "the query changed");
    assert!(
        filtered.len() < all,
        "and it actually filtered: {} of {all}",
        filtered.len()
    );
    assert_eq!(
        filtered,
        app.palette_entries_uncached(),
        "the memo is exact"
    );
}

#[test]
fn cycling_the_input_mode_re_derives_the_palette() {
    // Every entry carries the chord for the *active* mode, so the mode
    // is part of the key — a memo without it would show Doom chords in
    // Vim.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    let doom = app.palette_entries();
    app.run(&mut shell, "cycle-mode");
    let other = app.palette_entries();
    assert_eq!(other, app.palette_entries_uncached(), "exact after a cycle");
    let doom_chords: Vec<&str> = doom.iter().map(|e| e.action.chord()).collect();
    let other_chords: Vec<&str> = other.iter().map(|e| e.action.chord()).collect();
    assert_ne!(doom_chords, other_chords, "chords follow the mode");
}

#[test]
fn palette_navigation_does_not_rebuild_the_list() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "palette");
    let _ = app.palette_entries();
    let before = app.palette_recomputes();
    for _ in 0..10 {
        app.on_key(&mut shell, "down", false, false, None);
    }
    assert_eq!(
        app.palette_recomputes(),
        before,
        "moving the cursor changes no entries"
    );
}
