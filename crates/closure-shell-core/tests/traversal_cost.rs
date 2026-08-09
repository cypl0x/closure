//! "When holding j or k and going through all of the headlines quickly
//! it sometimes freezes for some milliseconds. Especially with vault
//! headers that have quite a tree attached to them."
//!
//! Moving the selection derives the detail pane for the row you land
//! on, and that read the *whole subtree* of the new headline: it
//! materialised every descendant's source into a string, stripped
//! property drawers over it, concatenated it to the body, and then
//! counted its lines and its words. Five passes over the subtree, per
//! keystroke, on the thread that draws the window.
//!
//! Which is why it is worst exactly where the report says: a headline
//! with a big tree under it. Landing on a leaf is free; landing on a
//! chapter is not.
//!
//! Measured as a shape rather than a clock — four times the subtree
//! must not cost four times as much, on any machine.
//!
//! What is left is one pass per *file*, the first time a detail is
//! wanted at a given vault revision: a prefix sum that answers every
//! headline in that file at once. Paid once per edit, not once per
//! keystroke, and only for the file you are actually in — which is
//! what [`an_edit_does_not_recount_the_files_you_are_not_in`] holds
//! to.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

/// A vault whose first headline carries `n` descendants, followed by
/// `n` shallow siblings to walk through.
fn vault_of(n: usize) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    let mut org = String::from("* Chapter\n:PROPERTIES:\n:ID: 01TRAV00000000000000000A\n:END:\n");
    for i in 0..n {
        let _ = write!(
            org,
            "** Section {i}\n:PROPERTIES:\n:ID: 01TRAVS{i:018}\n:END:\n\
             a paragraph of body text under section {i}, long enough to be worth counting\n"
        );
    }
    std::fs::write(dir.path().join("notes.org"), org).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

/// What it costs to walk the whole outline once, deriving the detail
/// at every step — which is what holding `j` does.
fn walk(shell: &Shell) -> Duration {
    let mut app = ModalApp::new(InputMode::Doom);
    let rows = app.rows(shell).len();
    // Warm *this* app: holding `j` happens in a running window, whose
    // first frame is long past. The one-time per-file pass belongs to
    // that first frame, and measuring it here would hide the thing
    // being measured behind it.
    app.select(0, shell);
    let _ = app.selected_detail(shell);
    let t = Instant::now();
    for i in 0..rows.min(40) {
        app.select(i, shell);
        // The render path's entry point: what the window asks for on
        // every frame after the selection moved.
        let _ = app.selected_detail(shell);
    }
    t.elapsed()
}

#[allow(clippy::cast_precision_loss)]
fn ratio(small: Duration, large: Duration) -> f64 {
    if small.as_nanos() == 0 {
        return 0.0;
    }
    large.as_nanos() as f64 / small.as_nanos() as f64
}

#[test]
fn landing_on_a_headline_does_not_cost_its_subtree() {
    let (_a, small) = vault_of(250);
    let (_b, large) = vault_of(1_000);
    let a = walk(&small);
    let b = walk(&large);
    let g = ratio(a, b);
    assert!(
        g < 2.0,
        "four times the subtree cost {g:.1} times as much to walk \
         ({a:?} -> {b:?}) — the detail is reading the whole tree on every step"
    );
}

#[test]
fn the_detail_still_says_what_it_said() {
    // A faster wrong answer is not the goal: the counts are of the
    // headline *and everything under it*, which is the whole reason
    // they were computed that way.
    let (_d, shell) = vault_of(3);
    let mut app = ModalApp::new(InputMode::Doom);
    app.select(0, &shell);
    let d = app.selected_detail(&shell).expect("a detail");
    assert!(
        d.words > 30,
        "the counts stopped including the subtree: {} words",
        d.words
    );
    assert!(d.lines >= 6, "{} lines", d.lines);
}

#[test]
fn a_leaf_is_still_counted_as_itself() {
    let (_d, shell) = vault_of(3);
    let mut app = ModalApp::new(InputMode::Doom);
    // The last row is a leaf section with one paragraph.
    let last = app.rows(&shell).len() - 1;
    app.select(last, &shell);
    let d = app.selected_detail(&shell).expect("a detail");
    assert!(d.words > 5 && d.words < 30, "{} words", d.words);
}

/// A vault with one small file to work in, plus `n` headlines of
/// somebody else's file sitting beside it.
fn vault_beside(n: usize) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("here.org"),
        "* Here\n:PROPERTIES:\n:ID: 01TRAVHERE00000000000000\n:END:\nthe file you are in\n",
    )
    .unwrap();
    let mut org = String::new();
    for i in 0..n {
        let _ = write!(
            org,
            "* Elsewhere {i}\n:PROPERTIES:\n:ID: 01TRAVE{i:018}\n:END:\n\
             a paragraph of body text you are not looking at\n"
        );
    }
    std::fs::write(dir.path().join("elsewhere.org"), org).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

/// What the first detail after an edit costs — the frame that pays for
/// the counting pass.
fn first_landing(shell: &Shell) -> Duration {
    let mut app = ModalApp::new(InputMode::Doom);
    let rows = app.rows(shell);
    let here = rows
        .iter()
        .position(|r| r.title == "Here")
        .expect("the file you are in");
    let t = Instant::now();
    app.select(here, shell);
    let _ = app.selected_detail(shell);
    t.elapsed()
}

#[test]
fn an_edit_does_not_recount_the_files_you_are_not_in() {
    // The pass is per file, so a vault four times the size costs the
    // same to land in — as long as the growth is somewhere else.
    let (_a, small) = vault_beside(250);
    let (_b, large) = vault_beside(1_000);
    let a = first_landing(&small);
    let b = first_landing(&large);
    let g = ratio(a, b);
    assert!(
        g < 2.0,
        "four times the vault cost {g:.1} times as much to land in one \
         small file of it ({a:?} -> {b:?}) — the counting pass is reading \
         files nobody asked about"
    );
}
