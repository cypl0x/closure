//! Three reports, one arc:
//!
//! - "collapsed properties make caret to disappear" — with a screenshot
//!   of a buffer showing lines 1, 2, 5, 6, 9, 10, 11 and no caret
//!   anywhere on it.
//! - "folded/collapsed properties can't be toggled with tab"
//! - "toggling heading collaps in editor is not possible anymore"
//!
//! A body buffer opens with its property drawers already folded, and
//! the caret is restored to wherever it was last time — which can be a
//! line inside one of them. A shell paints every line except the
//! hidden ones, so a caret on a hidden line is a caret that is not
//! drawn: it has not moved and it still takes your typing, which is
//! worse than losing it.
//!
//! Nothing may rest on a hidden line, then: not after folding, not
//! after opening, and not after moving. And the key that folds is
//! `TAB`, because that is `org-cycle` and every one of these reports
//! is somebody pressing it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Parent
:PROPERTIES:
:ID: 01HQFOLD00000000000001
:END:
** Serve Model Context Protocol
:PROPERTIES:
:ID: 01HQFOLD00000000000002
:CUSTOM: x
:END:
- Good fit with the LLM
- closure Agent harness
";

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQFOLD00000000000001"));
    app.run(&mut shell, "toggle-view");
    (dir, shell, app)
}

/// The line the caret is on.
fn caret_line(app: &ModalApp) -> usize {
    app.body_cursor().0
}

#[test]
fn a_buffer_never_opens_with_its_caret_on_a_hidden_line() {
    // The screenshot: drawers folded, no caret painted anywhere.
    let (_d, _sh, app) = editing();
    assert!(
        !app.body_hidden_lines().contains(&caret_line(&app)),
        "caret on line {} of hidden {:?}",
        caret_line(&app),
        app.body_hidden_lines()
    );
}

#[test]
fn tab_folds_and_unfolds_the_thing_at_point() {
    // org-cycle, which is what every one of the three reports is
    // somebody pressing.
    let (_d, mut shell, mut app) = editing();
    app.body_click(1, 0); // the `:PROPERTIES:` line
    let folded = app.body_hidden_lines().len();
    app.on_key(&mut shell, "tab", false, false, None);
    let after = app.body_hidden_lines().len();
    assert_ne!(folded, after, "TAB changed nothing");

    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(
        app.body_hidden_lines().len(),
        folded,
        "and TAB again put it back"
    );
}

#[test]
fn tab_folds_a_headline_too() {
    // "toggling heading collaps in editor is not possible anymore".
    let (_d, mut shell, mut app) = editing();
    app.body_click(0, 0); // the `* Parent` line
    let before = app.body_hidden_lines().len();
    app.on_key(&mut shell, "tab", false, false, None);
    assert!(
        app.body_hidden_lines().len() > before,
        "a headline folds its subtree: {:?}",
        app.body_hidden_lines()
    );
}

#[test]
fn folding_takes_the_caret_out_with_it() {
    let (_d, mut shell, mut app) = editing();
    app.body_click(2, 0); // inside the first drawer
    // Unfold, put the caret inside, fold again.
    app.on_key(&mut shell, "tab", false, false, None);
    app.body_click(2, 0);
    app.on_key(&mut shell, "tab", false, false, None);
    assert!(
        !app.body_hidden_lines().contains(&caret_line(&app)),
        "caret left on hidden line {} of {:?}",
        caret_line(&app),
        app.body_hidden_lines()
    );
}

#[test]
fn moving_down_steps_over_a_fold_rather_than_into_it() {
    let (_d, mut shell, mut app) = editing();
    app.body_click(0, 0);
    for _ in 0..4 {
        app.on_key(&mut shell, "down", false, false, None);
        assert!(
            !app.body_hidden_lines().contains(&caret_line(&app)),
            "landed on hidden line {} of {:?}",
            caret_line(&app),
            app.body_hidden_lines()
        );
    }
}

#[test]
fn moving_up_steps_over_one_too() {
    let (_d, mut shell, mut app) = editing();
    app.body_click(9, 0);
    for _ in 0..6 {
        app.on_key(&mut shell, "up", false, false, None);
        assert!(
            !app.body_hidden_lines().contains(&caret_line(&app)),
            "landed on hidden line {} of {:?}",
            caret_line(&app),
            app.body_hidden_lines()
        );
    }
}

#[test]
fn tab_still_indents_while_you_are_typing() {
    // org's TAB is `org-cycle` in NORMAL and something else entirely in
    // INSERT — taking it for folding everywhere would cost the thing
    // people press it for most.
    let (_d, mut shell, mut app) = editing();
    app.on_key(&mut shell, "i", false, false, Some('i'));
    let folds = app.body_hidden_lines().len();
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(
        app.body_hidden_lines().len(),
        folds,
        "TAB in INSERT is not a fold"
    );
}
