//! "dabbrev or autocompletion like in the editor for the
//! capture/rename/add/whatever prompt."
//!
//! The body editor has completed words against the vault for a while:
//! `C-n`/`C-p` cycle, TAB accepts, anything else ends the session. The
//! one-line prompts had nothing, so the vocabulary you had just typed
//! into a note was unavailable in the prompt that files the next one —
//! and the titles that most want completing are exactly the ones that
//! repeat.
//!
//! Same grammar, because a second grammar for the same idea is a thing
//! to remember: the editor's chords, over whichever prompt is open.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Refactoring the parser
:PROPERTIES:
:ID: 01HQCMP000000000000000001
:END:
Refactoring is the word this vault says a lot.
* Reference material
:PROPERTIES:
:ID: 01HQCMP000000000000000002
:END:
reference body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Type `s` into whichever prompt is open, one character at a time.
fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

fn next(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "n", true, false, None);
}

fn prev(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "p", true, false, None);
}

fn tab(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "tab", false, false, None);
}

#[test]
fn c_n_completes_the_word_being_typed_in_the_capture_prompt() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Refa");
    next(&mut app, &mut shell);

    assert_eq!(
        app.capture_buffer(),
        "Refactoring",
        "the vault's own word, completed in the prompt"
    );
}

#[test]
fn only_the_last_word_is_replaced() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "finish Refa");
    next(&mut app, &mut shell);

    assert_eq!(app.capture_buffer(), "finish Refactoring");
}

#[test]
fn c_n_and_c_p_walk_the_candidates_both_ways() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Ref");
    next(&mut app, &mut shell);
    let first = app.capture_buffer().to_owned();
    next(&mut app, &mut shell);
    let second = app.capture_buffer().to_owned();
    assert_ne!(first, second, "C-n stepped to another candidate");
    prev(&mut app, &mut shell);
    assert_eq!(app.capture_buffer(), first, "C-p came back");
}

#[test]
fn the_shell_can_paint_the_candidates() {
    // The popup beside the caret is the same one the editor gets: a
    // cycle you cannot see is a cycle you have to count.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Ref");
    assert!(app.prompt_completion_items().is_empty(), "nothing yet");

    next(&mut app, &mut shell);
    assert!(
        app.prompt_completion_items()
            .iter()
            .any(|i| i == "Refactoring"),
        "{:?}",
        app.prompt_completion_items()
    );
    assert_eq!(app.prompt_completion_ix(), Some(0));
}

#[test]
fn tab_completes_when_nothing_is_cycling_yet() {
    // TAB means "complete this" everywhere else on the desktop, and a
    // one-line title prompt has no indentation for it to mean instead.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Refa");
    tab(&mut app, &mut shell);

    assert_eq!(app.capture_buffer(), "Refactoring");
}

#[test]
fn tab_accepts_the_candidate_a_cycle_is_showing() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Refa");
    next(&mut app, &mut shell);
    tab(&mut app, &mut shell);

    assert_eq!(app.capture_buffer(), "Refactoring", "the text stands");
    assert!(
        app.prompt_completion_items().is_empty(),
        "and the popup is gone"
    );
}

#[test]
fn typing_ends_the_cycle() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Refa");
    next(&mut app, &mut shell);
    type_in(&mut app, &mut shell, "!");

    assert!(app.prompt_completion_items().is_empty());
    assert_eq!(app.capture_buffer(), "Refactoring!");
}

#[test]
fn a_prompt_on_a_word_boundary_has_nothing_to_complete() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Refa ");
    next(&mut app, &mut shell);

    assert_eq!(app.capture_buffer(), "Refa ", "untouched");
    assert!(app.prompt_completion_items().is_empty());
}

#[test]
fn the_rename_prompt_completes_too() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQCMP000000000000000002"));
    app.run(&mut shell, "rename");
    // Rename opens on the existing title; clear it and type a prefix.
    app.on_key(&mut shell, "u", true, false, None);
    type_in(&mut app, &mut shell, "Refa");
    next(&mut app, &mut shell);

    assert_eq!(app.field_buffer(), "Refactoring");
}

#[test]
fn the_add_sibling_prompt_completes_too() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQCMP000000000000000001"));
    app.run(&mut shell, "add-sibling");
    type_in(&mut app, &mut shell, "Refa");
    next(&mut app, &mut shell);

    assert_eq!(app.field_buffer(), "Refactoring");
}

#[test]
fn a_todo_keyword_is_offered_but_the_structural_ones_are_not() {
    // A title can start with TODO. It cannot usefully start with
    // `:PROPERTIES:`, which the body editor offers because a body is
    // where drawers live.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "TOD");
    next(&mut app, &mut shell);
    assert_eq!(app.capture_buffer(), "TODO");

    let (_d2, mut shell2, mut app2) = fixture();
    app2.run(&mut shell2, "capture-start");
    type_in(&mut app2, &mut shell2, "PROP");
    next(&mut app2, &mut shell2);
    assert!(
        !app2.capture_buffer().contains(":PROPERTIES:"),
        "{}",
        app2.capture_buffer()
    );
}

#[test]
fn a_candidate_can_be_picked_by_index() {
    // The strip is on screen, so the mouse gets the same say the
    // chords do — the shells click a row rather than synthesising the
    // right number of `C-n`s.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Ref");
    next(&mut app, &mut shell);
    let items: Vec<String> = app.prompt_completion_items().to_vec();
    assert!(items.len() > 1, "{items:?}");

    app.pick_prompt_completion(1);
    assert_eq!(app.capture_buffer(), items[1]);
    assert!(
        app.prompt_completion_items().is_empty(),
        "picking one is the end of the cycle"
    );
}

#[test]
fn picking_past_the_end_does_nothing() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture-start");
    type_in(&mut app, &mut shell, "Ref");
    next(&mut app, &mut shell);
    let before = app.capture_buffer().to_owned();
    app.pick_prompt_completion(99);
    assert_eq!(app.capture_buffer(), before);
}
