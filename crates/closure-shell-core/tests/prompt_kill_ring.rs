//! What `C-k` takes out of a prompt has to be recoverable.
//!
//! Reported 2026-08-02: "ctlr+k in the capture/add sibling/whatever
//! prompt will kill until the end of the line, but the kill is not
//! accessible anywhere. In the editor view when I kill with ctlr+k in
//! INSERT mode I am able to paste via ctlr+y in INSERT mode or p in
//! NORMAL mode. This isn't possible in the capture etc. prompts."
//!
//! The one-line fields dropped the text on the floor, so `C-k` was a
//! delete wearing a kill's name.
//!
//! `C-k` in the *capture* prompt is deliberately not this: there it
//! walks the capture history, which is a older and better use of the
//! chord in a field with a history behind it. The kill chords the
//! report is about are the field prompts' — rename, add, tags,
//! property — plus `C-w` and `C-u`, which every field has.
//!
//! The prompts share a kill of their own rather than the vault's kill
//! ring. That ring holds org *subtrees* — it is what `p` splices back
//! into the outline — and pushing a fragment of a title onto it would
//! make the next `p` paste prose where a headline belongs.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQPKR0000000000000001\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn type_into(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
}

/// The rename prompt, cleared and ready to type into.
fn rename_prompt(app: &mut ModalApp, sh: &mut Shell) {
    app.select(0, sh);
    app.run(sh, "rename");
    assert_eq!(app.surface(), ModalSurface::Rename);
    app.on_key(sh, "u", true, false, None); // C-u clears the prefill
}

#[test]
fn ctrl_k_then_ctrl_y_puts_it_back() {
    let (_d, mut sh, mut app) = fixture();
    rename_prompt(&mut app, &mut sh);
    type_into(&mut app, &mut sh, "hello brave world");
    app.on_key(&mut sh, "a", true, false, None); // C-a, to the start
    app.on_key(&mut sh, "k", true, false, None); // C-k, kill the line
    assert_eq!(app.field_buffer(), "", "killed");
    app.on_key(&mut sh, "y", true, false, None); // C-y, yank it back
    assert_eq!(app.field_buffer(), "hello brave world");
}

#[test]
fn ctrl_k_in_the_capture_prompt_is_still_its_history() {
    // Capture has a history and `C-j`/`C-k` walk it — a use of the
    // chord that predates this and belongs to a field that has one.
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "capture");
    type_into(&mut app, &mut sh, "first thought");
    app.on_key(&mut sh, "enter", false, false, None);
    app.run(&mut sh, "capture");
    app.on_key(&mut sh, "k", true, false, None);
    assert_eq!(app.capture_buffer(), "first thought", "history, not a kill");
}

#[test]
fn the_kill_survives_the_trip_to_another_prompt() {
    // Killing in one field and yanking in another is the whole point of
    // a kill being a *kill*.
    let (_d, mut sh, mut app) = fixture();
    rename_prompt(&mut app, &mut sh);
    type_into(&mut app, &mut sh, "carried across");
    app.on_key(&mut sh, "a", true, false, None);
    app.on_key(&mut sh, "k", true, false, None);
    app.on_key(&mut sh, "escape", false, false, None);
    app.run(&mut sh, "capture");
    app.on_key(&mut sh, "y", true, false, None);
    assert_eq!(app.capture_buffer(), "carried across");
}

#[test]
fn ctrl_u_kills_too() {
    let (_d, mut sh, mut app) = fixture();
    rename_prompt(&mut app, &mut sh);
    type_into(&mut app, &mut sh, "front and back");
    app.on_key(&mut sh, "u", true, false, None); // kill to start
    assert_eq!(app.field_buffer(), "");
    app.on_key(&mut sh, "y", true, false, None);
    assert_eq!(app.field_buffer(), "front and back");
}

#[test]
fn the_word_kill_is_a_kill_as_well() {
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "capture");
    type_into(&mut app, &mut sh, "keep this");
    app.on_key(&mut sh, "w", true, false, None); // C-w
    assert_eq!(app.capture_buffer(), "keep ");
    app.on_key(&mut sh, "y", true, false, None);
    assert_eq!(app.capture_buffer(), "keep this");
}

#[test]
fn yanking_lands_at_the_cursor_not_at_the_end() {
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "capture");
    type_into(&mut app, &mut sh, "abc");
    app.on_key(&mut sh, "w", true, false, None); // kill "abc"
    type_into(&mut app, &mut sh, "xy");
    app.on_key(&mut sh, "left", false, false, None);
    app.on_key(&mut sh, "y", true, false, None);
    assert_eq!(app.capture_buffer(), "xabcy");
}

#[test]
fn a_yank_with_nothing_killed_changes_nothing() {
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "capture");
    type_into(&mut app, &mut sh, "typed");
    app.on_key(&mut sh, "y", true, false, None);
    assert_eq!(app.capture_buffer(), "typed");
}

#[test]
fn a_prompt_kill_never_reaches_the_subtree_ring() {
    // `p` splices the vault's ring back into the outline as org source.
    // A fragment of a title on that ring would paste prose where a
    // headline belongs.
    let (_d, mut sh, mut app) = fixture();
    rename_prompt(&mut app, &mut sh);
    type_into(&mut app, &mut sh, "not a subtree");
    app.on_key(&mut sh, "a", true, false, None);
    app.on_key(&mut sh, "k", true, false, None);
    assert!(sh.ring_top().is_none(), "{:?}", sh.ring_top());
}
