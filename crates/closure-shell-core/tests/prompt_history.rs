//! "prompt history (toggle by action or not) — This is useful when
//! ESC'ing a long prompt by accident and if I am able to recover from
//! this due to the prompt history. This is useful"
//!
//! The palette kept a history of the commands you ran; nothing else
//! kept anything. A capture you had typed three sentences into and
//! then dismissed with `Esc` was gone, and `Esc` is one key away from
//! everything.
//!
//! So every prompt remembers, and — this is the whole point of the
//! item — it remembers what you *abandoned* as well as what you
//! accepted. A history that only recorded successes would be a history
//! that forgets exactly the case the report is about.
//!
//! Per kind, not one shared ring: the last thing you renamed something
//! to is not a candidate for the tag prompt, and a single list would
//! make every prompt's history mostly other prompts.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQHIST00000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQHIST00000000000001"));
    (dir, shell, app)
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

/// `M-p` — the previous entry. Emacs's own minibuffer history key, and
/// it has to be that rather than `C-p`: `C-n`/`C-p` in a prompt are
/// already the completion cycle, and in a picker they walk the list.
fn prev(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "p", false, true, None);
}

/// `M-n` — the next one.
fn next(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "n", false, true, None);
}

#[test]
fn a_prompt_abandoned_by_accident_can_be_got_back() {
    // The report, exactly: three sentences into a capture, `Esc`.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture");
    type_in(
        &mut app,
        &mut shell,
        "a long thought I did not mean to lose",
    );
    app.on_key(&mut shell, "escape", false, false, None);

    app.run(&mut shell, "capture");
    prev(&mut app, &mut shell);
    assert_eq!(
        app.prompt_text(),
        Some("a long thought I did not mean to lose")
    );
}

#[test]
fn what_was_accepted_is_remembered_too() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "rename");
    // The rename prompt opens pre-filled; clear it first.
    for _ in 0..40 {
        app.on_key(&mut shell, "backspace", false, false, None);
    }
    type_in(&mut app, &mut shell, "Renamed once");
    app.on_key(&mut shell, "enter", false, false, None);
    // A second rename, so that recalling the first is a real recall
    // rather than the prompt's own pre-filled title read back.
    app.run(&mut shell, "rename");
    for _ in 0..40 {
        app.on_key(&mut shell, "backspace", false, false, None);
    }
    type_in(&mut app, &mut shell, "Renamed twice");
    app.on_key(&mut shell, "enter", false, false, None);

    app.run(&mut shell, "rename");
    prev(&mut app, &mut shell);
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("Renamed once"));
}

#[test]
fn the_history_walks_both_ways() {
    let (_d, mut shell, mut app) = fixture();
    for text in ["first", "second"] {
        app.run(&mut shell, "capture");
        type_in(&mut app, &mut shell, text);
        app.on_key(&mut shell, "escape", false, false, None);
    }
    app.run(&mut shell, "capture");
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("second"), "newest first");
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("first"));
    next(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("second"));
}

#[test]
fn walking_past_the_newest_gives_back_what_you_were_typing() {
    // The thing every shell does, and the reason walking history is
    // safe: your own half-typed line is not lost by looking.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture");
    type_in(&mut app, &mut shell, "remembered");
    app.on_key(&mut shell, "escape", false, false, None);

    app.run(&mut shell, "capture");
    type_in(&mut app, &mut shell, "half typed");
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("remembered"));
    next(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("half typed"), "given back");
}

#[test]
fn each_prompt_keeps_its_own() {
    // The last thing you renamed something to is not a candidate for a
    // capture, and one shared ring would make every prompt's history
    // mostly other prompts.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture");
    type_in(&mut app, &mut shell, "a captured thing");
    app.on_key(&mut shell, "escape", false, false, None);

    app.run(&mut shell, "rename");
    for _ in 0..40 {
        app.on_key(&mut shell, "backspace", false, false, None);
    }
    prev(&mut app, &mut shell);
    assert_ne!(
        app.prompt_text(),
        Some("a captured thing"),
        "the capture leaked into the rename prompt"
    );
}

#[test]
fn an_empty_prompt_is_not_worth_remembering() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "capture");
    app.on_key(&mut shell, "escape", false, false, None);
    app.run(&mut shell, "capture");
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some(""), "nothing to recall");
}

#[test]
fn the_same_text_twice_is_one_entry() {
    let (_d, mut shell, mut app) = fixture();
    for _ in 0..3 {
        app.run(&mut shell, "capture");
        type_in(&mut app, &mut shell, "same");
        app.on_key(&mut shell, "escape", false, false, None);
    }
    app.run(&mut shell, "capture");
    prev(&mut app, &mut shell);
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("same"), "one entry, not three");
}

#[test]
fn the_search_prompt_has_one_as_well() {
    // "history everywhere": a filter you spent thought on is worth as
    // much as a title you typed.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "search");
    type_in(&mut app, &mut shell, "alpha");
    app.on_key(&mut shell, "escape", false, false, None);

    app.run(&mut shell, "search");
    assert_eq!(app.surface(), ModalSurface::Search);
    prev(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("alpha"));
}
