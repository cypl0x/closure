//! The outline row memo.
//!
//! `ModalApp::rows` walks every document in the vault and allocates
//! five strings per headline. The gpui render path calls it from the
//! context line, the outline pane, the detail pane and from inside
//! every mouse listener — a dozen full vault walks per frame, which is
//! where the reference shell's input lag came from.
//!
//! Invariants under test:
//!  1. Repeated calls with nothing changed recompute exactly once.
//!  2. The memo is *exact*: any change that could alter the row list —
//!     a vault mutation, a fold, a surface switch, a search query
//!     keystroke — produces the same rows an uncached walk would.
//!
//! Invariant 2 is the one that matters; a stale cache is a correctness
//! bug, not a performance one, so every case asserts against a freshly
//! computed reference.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Row, Shell};
use closure_store::Vault;

const SRC: &str = "* TODO Alpha\n:PROPERTIES:\n:ID: 01HQROW00000000000000001\n:END:\n\
                   ** Beta\n:PROPERTIES:\n:ID: 01HQROW00000000000000002\n:END:\n\
                   * Gamma\n:PROPERTIES:\n:ID: 01HQROW00000000000000003\n:END:\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Recompute the row list from scratch, bypassing the memo, so the
/// tests below compare against ground truth rather than themselves.
fn uncached(app: &ModalApp, shell: &Shell) -> Vec<Row> {
    app.rows_uncached(shell)
}

#[test]
fn repeated_calls_recompute_once() {
    let (_d, shell, app) = fixture();
    let before = app.rows_recomputes();
    for _ in 0..25 {
        assert_eq!(app.rows(&shell).len(), 3);
    }
    assert_eq!(
        app.rows_recomputes() - before,
        1,
        "25 identical row queries must cost one vault walk"
    );
}

#[test]
fn a_vault_mutation_invalidates_the_memo() {
    let (_d, mut shell, app) = fixture();
    assert_eq!(app.rows(&shell).len(), 3);
    let after_warm = app.rows_recomputes();

    shell
        .add_sibling(
            &closure_core::BlockId::from_existing("01HQROW00000000000000003"),
            "Delta",
        )
        .expect("add");

    let rows = app.rows(&shell);
    assert_eq!(
        rows,
        uncached(&app, &shell),
        "memo is exact after a mutation"
    );
    assert_eq!(rows.len(), 4, "the new sibling shows up");
    assert!(
        app.rows_recomputes() > after_warm,
        "the mutation must force a recompute"
    );
}

#[test]
fn renaming_through_the_shell_is_reflected() {
    let (_d, mut shell, app) = fixture();
    assert_eq!(app.rows(&shell)[0].title, "Alpha");
    shell
        .rename_headline(
            &closure_core::BlockId::from_existing("01HQROW00000000000000001"),
            "Renamed",
        )
        .expect("rename");
    assert_eq!(
        app.rows(&shell)[0].title,
        "Renamed",
        "a stale memo would still say Alpha"
    );
    assert_eq!(app.rows(&shell), uncached(&app, &shell));
}

#[test]
fn folding_changes_the_row_set() {
    let (_d, mut shell, mut app) = fixture();
    assert_eq!(app.rows(&shell).len(), 3);
    // Select Alpha and fold it: Beta (its child) must disappear.
    app.select(0, &shell);
    app.run(&mut shell, "toggle-fold");
    let rows = app.rows(&shell);
    assert_eq!(rows.len(), 2, "folded Alpha hides Beta: {rows:?}");
    assert_eq!(rows, uncached(&app, &shell), "memo is exact after a fold");
}

#[test]
fn the_search_query_is_part_of_the_key() {
    let (_d, mut shell, mut app) = fixture();
    assert_eq!(app.rows(&shell).len(), 3);
    app.run(&mut shell, "search-start");
    // Each keystroke narrows the set; a memo keyed only on the vault
    // revision would freeze the list at "all three".
    for (c, expect) in [('B', 1usize), ('e', 1), ('t', 1)] {
        app.on_key(&mut shell, "b", false, false, Some(c));
        let rows = app.rows(&shell);
        assert_eq!(rows.len(), expect, "query {:?} -> {rows:?}", app.query());
        assert_eq!(rows, uncached(&app, &shell));
    }
}

#[test]
fn leaving_the_search_surface_restores_every_row() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "search-start");
    app.on_key(&mut shell, "b", false, false, Some('B'));
    assert_eq!(app.rows(&shell).len(), 1);
    app.on_key(&mut shell, "escape", false, false, None);
    let rows = app.rows(&shell);
    assert_eq!(rows.len(), 3, "the surface switch is part of the key");
    assert_eq!(rows, uncached(&app, &shell));
}

#[test]
fn undo_invalidates_the_memo() {
    let (dir, mut shell, app) = fixture();
    shell
        .rename_headline(
            &closure_core::BlockId::from_existing("01HQROW00000000000000001"),
            "Renamed",
        )
        .expect("rename");
    assert_eq!(app.rows(&shell)[0].title, "Renamed");
    shell
        .vault
        .undo_in(&dir.path().join("notes.org"))
        .expect("undo");
    assert_eq!(
        app.rows(&shell)[0].title,
        "Alpha",
        "undo restores the title; the memo must notice"
    );
}

#[test]
fn the_memo_keeps_dispatch_off_the_vault_walk() {
    // The real payoff: a navigation keystroke used to walk the vault
    // several times over (bounds clamp, selection, status). With the
    // memo warm it must walk at most once per key.
    let (_d, mut shell, mut app) = fixture();
    let _ = app.rows(&shell);
    let before = app.rows_recomputes();
    for _ in 0..10 {
        app.on_key(&mut shell, "j", false, false, Some('j'));
    }
    assert_eq!(
        app.rows_recomputes(),
        before,
        "pure navigation changes no rows, so it must not recompute them"
    );
}
