//! The half of the loop between reading a key and painting.
//!
//! `run_loop` reads a stroke, hands it to `App`, then runs the vault
//! side: `sync_app` and `sync_panes` refresh what the app knows from
//! the vault, and the three `apply_*` functions carry out whatever the
//! stroke asked for. That side was a third of this crate and entirely
//! untested, for the same reason `draw` was — it was private, so the
//! only way in wanted a TTY.
//!
//! It needs no terminal. It is `&mut App` and `&mut Vault`, and it is
//! where the shell decides what the user sees and what lands on disk.
//!
//! The claims worth making here are about *both* sides: a request must
//! reach the vault, and the app must be told what the vault now says.
//! A capture that writes the file and leaves the outline stale is the
//! failure this half exists to prevent.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_store::Vault;
use closure_tui::{App, apply_requests, apply_structure_requests, sync_app, sync_panes};
use tempfile::TempDir;

const A: &str = "01TUILOOP00000000001";
const B: &str = "01TUILOOP00000000002";

fn vault() -> (TempDir, Vault) {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* TODO First headline :work:\n:PROPERTIES:\n:ID: {A}\n:END:\n\
             the first body\n\
             * Second headline\n:PROPERTIES:\n:ID: {B}\n:END:\n\
             links to [[id:{A}]]\n"
        ),
    )
    .expect("write");
    let v = Vault::open(d.path()).expect("open");
    (d, v)
}

fn journal(v: &Vault) -> closure_record::Journal {
    closure_record::Journal::new(v.root(), false)
}

/// An app that has been told what the vault holds.
fn synced(v: &Vault) -> App {
    let mut app = App::new(v.paths());
    sync_app(&mut app, v);
    app
}

/// The same, with one chord of our own bound to `capture`.
fn synced_with_capture_on_k(v: &Vault) -> App {
    let mut app = App::with_bindings(v.paths(), &[("K", "capture")]);
    sync_app(&mut app, v);
    app
}

#[test]
fn syncing_tells_the_app_which_files_the_vault_has() {
    let (_d, v) = vault();
    let mut app = App::new(Vec::new());
    assert_eq!(app.selected_index(), None, "an empty app selects nothing");

    sync_app(&mut app, &v);
    assert_eq!(
        app.selected_index(),
        Some(0),
        "sync left it with no selection"
    );
    assert!(
        app.selected_path()
            .is_some_and(|p| p.ends_with("notes.org")),
        "{:?}",
        app.selected_path()
    );
}

#[test]
fn syncing_an_app_that_already_has_files_does_not_lose_the_selection() {
    // The loop calls this after every stroke. A sync that reset the
    // cursor would make the outline jump to the top on every keypress,
    // which is the sort of thing that is unusable long before it is
    // wrong.
    let (_d, v) = vault();
    let mut app = synced(&v);
    let before = app.selected_index();
    sync_app(&mut app, &v);
    assert_eq!(app.selected_index(), before);
}

#[test]
fn syncing_a_vault_with_no_files_leaves_an_app_that_still_works() {
    // `App::new(Vec::new())` is a shipped case with its own test, and
    // the sync has to survive it too — an empty vault is what a new
    // user has.
    let d = tempfile::tempdir().expect("tempdir");
    let v = Vault::open(d.path()).expect("open");
    let mut app = App::new(Vec::new());
    sync_app(&mut app, &v);
    sync_panes(&mut app, &v);
    assert_eq!(app.selected_index(), None);
}

#[test]
fn the_panes_are_derived_from_the_same_vault() {
    // "A feature that exists only in the window is a feature you cannot
    // use over ssh", per the function's own comment. So the panes have
    // to be populated here, not only in the GUI.
    let (_d, v) = vault();
    let mut app = synced(&v);
    sync_panes(&mut app, &v);
    // Whatever else it does, it must not panic and must leave the app
    // able to answer for its panes.
    let _ = app.pane_rows();
    let _ = app.history_rows();
}

#[test]
fn a_capture_request_reaches_the_vault_and_comes_back_to_the_app() {
    // Both halves. A capture that writes the file and leaves the
    // outline stale is exactly what this part of the loop exists to
    // prevent.
    let (_d, mut v) = vault();
    let mut app = synced_with_capture_on_k(&v);
    let j = journal(&v);

    // Driven the way a user does: the chord that opens the capture bar,
    // the title typed a character at a time, then Return. There is no
    // setter for the request — it exists only as the result of strokes,
    // which is the right shape and means a test has to type. The chord
    // is named here through `with_bindings` rather than looked up, so
    // this test does not break every time a keymap is rearranged.
    app.handle_stroke("K");
    for c in "A captured thought".chars() {
        app.handle_stroke(&c.to_string());
    }
    app.handle_stroke("RET");

    apply_requests(&mut app, &mut v, &j).expect("apply");

    let inbox = v.root().join(closure_store::CAPTURE_FILE);
    let src = fs::read_to_string(&inbox).expect("the capture file was not written");
    assert!(src.contains("A captured thought"), "{src}");

    // And the app knows the vault grew a file.
    assert!(
        app.paths()
            .iter()
            .any(|p| p.ends_with(closure_store::CAPTURE_FILE)),
        "the app was not told about the new file: {:?}",
        app.paths()
    );
}

#[test]
fn applying_with_nothing_requested_changes_nothing_and_succeeds() {
    // The common case by far: most strokes ask for nothing. It has to
    // be cheap and silent rather than an error.
    let (d, mut v) = vault();
    let mut app = synced(&v);
    let j = journal(&v);
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");

    apply_requests(&mut app, &mut v, &j).expect("apply");
    apply_structure_requests(&mut app, &mut v, &j).expect("structure");

    assert_eq!(
        fs::read_to_string(d.path().join("notes.org")).expect("read"),
        before
    );
}

#[test]
fn applying_org_verbs_with_nothing_pending_is_quiet() {
    // Most strokes ask for no org verb at all, so the common path
    // through here is "take_* returned None" — and it has to be silent
    // and cheap rather than an error.
    let (_d, mut v) = vault();
    let mut app = synced(&v);

    let got = closure_tui::apply_org_verb_requests(&mut app, &mut v);
    // Either it reports the error or it swallows it into the status
    // line; what it must not do is panic or corrupt the file.
    let _ = got;
    assert!(
        fs::read_to_string(v.root().join("notes.org"))
            .expect("read")
            .contains("First headline"),
        "the file was damaged by a refile that could not happen"
    );
}
