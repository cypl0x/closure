//! I11: what one keystroke makes the kernel derive costs no more than
//! [`closure_shell_core::KEYSTROKE_BUDGET_US`].
//!
//! This is the budget with a history. Moving the selection used to
//! derive the detail pane by reading the *whole subtree* of whatever
//! headline it landed on — materialise every descendant's source,
//! strip drawers over it, concatenate, count lines, count words. Five
//! passes over a chapter, per keystroke, on the thread that draws the
//! window, which is "holding j freezes for some milliseconds,
//! especially with headers that have quite a tree attached".
//!
//! `traversal_cost.rs` holds the *shape* — four times the subtree must
//! not cost four times as much — and that is the sensitive test. This
//! is the other kind: an absolute ceiling, loose enough to be portable,
//! that says the work has changed kind rather than degree.
//!
//! Deliberately not a frame budget. What a frame costs is the GPU's
//! business and this machine has no Vulkan driver, so a number measured
//! here would describe lavapipe rather than closure.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;
use std::time::Instant;

use closure_config::InputMode;
use closure_shell_core::{KEYSTROKE_BUDGET_US, ModalApp, Shell};
use closure_store::Vault;

/// A vault whose first headline has a large subtree under it — the
/// case the report was about — inside a vault of a realistic size.
fn shell_with_a_chapter(sections: usize) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    let mut org = String::from("* Chapter\n:PROPERTIES:\n:ID: 01BUDGETKEY0000000000000\n:END:\n");
    for i in 0..sections {
        let _ = write!(
            org,
            "** Section {i}\n:PROPERTIES:\n:ID: 01BUDGETK{i:017}\n:END:\n\
             a paragraph of body text under section {i}, long enough to be worth counting\n"
        );
    }
    std::fs::write(dir.path().join("notes.org"), org).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn a_keystroke_stays_inside_its_budget() {
    const STEPS: usize = 200;
    let (_dir, shell) = shell_with_a_chapter(5_000);
    let mut app = ModalApp::new(InputMode::Doom);
    // Warm: the first detail at a given revision pays for the file's
    // counting pass, which is per edit and not per keystroke.
    app.select(0, &shell);
    let _ = app.selected_detail(&shell);
    let t = Instant::now();
    for i in 0..STEPS {
        app.select(i, &shell);
        let _ = app.selected_detail(&shell);
    }
    let per = t.elapsed().as_micros() as f64 / STEPS as f64;
    assert!(
        per <= f64::from(KEYSTROKE_BUDGET_US),
        "one keystroke derived {per:.1} µs of work, budget is \
         {KEYSTROKE_BUDGET_US} (I11) — landing on a headline is reading \
         something it should not"
    );
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn the_budget_holds_where_the_report_said_it_would_not() {
    // Specifically on the chapter: landing on a headline with a large
    // subtree must cost what landing on a leaf costs.
    let (_dir, shell) = shell_with_a_chapter(5_000);
    let mut app = ModalApp::new(InputMode::Doom);
    app.select(1, &shell);
    let _ = app.selected_detail(&shell);
    let t = Instant::now();
    app.select(0, &shell);
    let _ = app.selected_detail(&shell);
    let per = t.elapsed().as_micros() as f64;
    assert!(
        per <= f64::from(KEYSTROKE_BUDGET_US),
        "landing on a headline with 5000 descendants took {per:.1} µs, \
         budget is {KEYSTROKE_BUDGET_US} (I11)"
    );
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn what_the_chapter_costs_does_not_grow_with_the_chapter() {
    // Landing on a headline with a big subtree costs about ten times
    // what landing on a leaf costs, and the interesting question is
    // whether that ten is a constant or a walk. It is a constant: the
    // preview materialises up to `PREVIEW_CAP` lines of the subtree and
    // a leaf has two, which is the cap doing exactly its job.
    //
    // Measured 2026-08-11 at 110 µs, 95 µs and 96 µs for subtrees of
    // 1,000, 5,000 and 20,000 sections. Twenty times the subtree, the
    // same cost — so this holds the shape rather than the number.
    let cost = |sections: usize| {
        let (_dir, shell) = shell_with_a_chapter(sections);
        let mut app = ModalApp::new(InputMode::Doom);
        // Warm the file's counting pass, which is per edit, not per
        // keystroke.
        app.select(1, &shell);
        let _ = app.selected_detail(&shell);
        // Alternate on and off the chapter so the detail memo misses
        // every time and each step really derives.
        let t = Instant::now();
        for i in 0..20 {
            app.select(usize::from(i % 2 != 0), &shell);
            let _ = app.selected_detail(&shell);
        }
        t.elapsed().as_micros() as f64 / 20.0
    };
    let small = cost(1_000);
    let large = cost(20_000);
    let growth = large / small.max(0.001);
    assert!(
        growth < 2.0,
        "twenty times the subtree cost {growth:.1} times as much to land \
         on ({small:.1} µs -> {large:.1} µs) — the preview is reading past \
         its cap"
    );
}
