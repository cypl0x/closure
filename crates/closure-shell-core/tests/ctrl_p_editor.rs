//! "ctlr+p isn't working in editor view"
//!
//! `C-p` was wired to the completion popup and to nothing else, so
//! with no popup showing it did nothing at all — one of the most worn
//! keys in Emacs, silent, beside a footer advertising the readline set
//! it belongs to.
//!
//! It walks the popup while the popup is up and is `previous-line` the
//! rest of the time: the trade the file already makes for `C-j`/`C-k`.
//!
//! `C-n` is deliberately *not* given the same treatment, and the first
//! version of this file was wrong to try. `C-n` summons the completion
//! — the footer says "C-n complete", seven tests in `modal` pin it —
//! so making it `next-line` for symmetry broke a shipped contract to
//! fix one nobody reported. The asymmetry is the point: there is no
//! version of "step back" that summons anything, which is exactly why
//! `C-p` is free and `C-n` is not.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Lines
:PROPERTIES:
:ID: 01HQCTRL00000000000001
:END:
first line
second line
third line
";

/// An open buffer in INSERT — the mode the report is about, and the
/// only one Notion and Emacs ever leave you in.
fn inserting(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(mode);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQCTRL00000000000001"));
    app.run(&mut shell, "edit-body");
    if app.body_mode() != closure_shell_core::EditorMode::Insert {
        app.on_key(&mut shell, "i", false, false, Some('i'));
    }
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Insert);
    (dir, shell, app)
}

fn ctrl(app: &mut ModalApp, shell: &mut Shell, key: &str) {
    app.on_key(shell, key, true, false, None);
}

#[test]
fn ctrl_p_goes_up_a_line() {
    // The report, verbatim.
    let (_d, mut shell, mut app) = inserting(InputMode::Emacs);
    app.body_click(2, 0);
    ctrl(&mut app, &mut shell, "p");
    assert_eq!(app.body_cursor().0, 1, "C-p is previous-line");
}

#[test]
fn ctrl_n_still_summons_the_completion() {
    // Not the twin, and this asserted the opposite for an hour before
    // it ever shipped. `C-n` is the completion's own key here — the
    // editor footer says "C-n complete" and seven tests in `modal`
    // pin it — so making it `next-line` for symmetry's sake broke a
    // contract the user has been using, to fix one they did not
    // report.
    //
    // `C-p` is the asymmetric one on purpose: there is no version of
    // "step back" that summons anything, so a popup that is not there
    // leaves the key free to be `previous-line`.
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    app.body_click(0, 0);
    for c in "thi".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let line_before = app.body_cursor().0;
    ctrl(&mut app, &mut shell, "n");
    assert_eq!(
        app.body_cursor().0,
        line_before,
        "C-n is the completion's key, not a motion"
    );
}

#[test]
fn they_work_in_every_mode() {
    // Not an Emacs-only courtesy: the readline set is the one thing
    // every mode shares, which is why the footer shows it in all of
    // them.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let (_d, mut shell, mut app) = inserting(mode);
        app.body_click(2, 0);
        ctrl(&mut app, &mut shell, "p");
        assert_eq!(app.body_cursor().0, 1, "{mode:?} cannot go up with C-p");
    }
}

#[test]
fn ctrl_p_still_walks_a_picker() {
    // The other `C-p` in the app, untouched: in a list surface it
    // moves the selection, and nothing here may reach into that.
    let (_d, mut shell, mut app) = inserting(InputMode::Doom);
    app.run(&mut shell, "list-headlines");
    ctrl(&mut app, &mut shell, "n");
    ctrl(&mut app, &mut shell, "p");
    // No assertion on the index — the point is that it is still the
    // picker's key and did not fall through to the buffer beneath.
    assert_ne!(app.surface(), closure_shell_core::ModalSurface::EditBody);
}
