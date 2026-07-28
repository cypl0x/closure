//! Knowing whether the body editor has something the vault does not.
//!
//! Nothing in the shell tracked it: `should_quit` was a bare flag, so
//! `:q` in the middle of an edit threw the buffer away without a word,
//! and the shells had nothing to put a modified marker on. Losing text
//! silently is the one failure a note-taking tool does not get to
//! make.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\nbody line\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

fn typ(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
}

/// The app parked in the body editor, INSERT, nothing typed yet.
fn editing() -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.run(&mut sh, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    (d, sh, app)
}

// === the dirty flag ===

#[test]
fn an_untouched_editor_is_clean() {
    let (_d, _sh, app) = editing();
    assert!(!app.body_dirty(), "opening an editor changes nothing");
}

#[test]
fn a_typed_character_makes_it_dirty() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "x");
    assert!(app.body_dirty());
}

#[test]
fn committing_makes_it_clean_again() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "x");
    app.commit_edit_body(&mut sh);
    assert!(!app.body_dirty(), "the vault has it now");
}

#[test]
fn typing_and_undoing_back_to_the_start_is_clean() {
    // The flag is a comparison, not a "was touched" bit: a buffer the
    // user has put back the way they found it has nothing to save.
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "x");
    app.on_key(&mut sh, "escape", false, false, None);
    typ(&mut app, &mut sh, "u");
    assert!(!app.body_dirty(), "back to what the vault holds");
}

#[test]
fn a_surface_that_is_not_the_editor_is_never_dirty() {
    let (_d, _sh) = shell();
    let app = ModalApp::new(InputMode::Vim);
    assert!(!app.body_dirty());
}

// === `:q` will not throw it away ===

#[test]
fn plain_q_refuses_to_quit_over_an_unsaved_body() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "new text");
    app.on_key(&mut sh, "escape", false, false, None);
    app.run_ex_line(&mut sh, "q");
    assert!(!app.should_quit(), "the buffer would have gone with it");
    assert!(
        app.status().contains("unsaved"),
        "and it says so: {:?}",
        app.status()
    );
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
}

// Contract revised 2026-07-28: from inside a buffer, `:q` and its
// bang close the *buffer* — vim's rule, where `:q` closes the window
// and quits only when it was the last one. `:qa` is the whole app from
// anywhere, and carries the same unsaved guard.

#[test]
fn bang_q_throws_the_buffer_away_anyway() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "throwaway");
    app.run_ex_line(&mut sh, "q!");
    assert_ne!(
        app.surface(),
        ModalSurface::EditBody,
        "the bang is the whole point"
    );
    assert!(!app.should_quit(), "of the buffer, not of the session");
    assert_eq!(app.unsaved_bodies(), 0, "and it is not held either");
}

#[test]
fn bang_qa_quits_anyway() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "throwaway");
    app.run_ex_line(&mut sh, "qa!");
    assert!(app.should_quit(), "the bang is the whole point");
}

#[test]
fn wq_saves_and_closes_the_buffer() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "kept");
    app.run_ex_line(&mut sh, "wq");
    assert_ne!(app.surface(), ModalSurface::EditBody);
    assert!(!app.body_dirty());
    assert!(
        sh.vault.iter().any(|(_, d)| d.source().contains("kept")),
        "written on the way out"
    );
}

#[test]
fn q_over_a_clean_editor_just_closes_it() {
    let (_d, mut sh, mut app) = editing();
    app.run_ex_line(&mut sh, "q");
    assert_ne!(app.surface(), ModalSurface::EditBody, "nothing to lose");
    assert!(!app.should_quit());
}

#[test]
fn the_quit_command_is_guarded_too() {
    // The palette, the chord and `:quit` reach the same command; the
    // guard cannot live in the ex line alone.
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "unsaved");
    app.on_key(&mut sh, "escape", false, false, None);
    app.run(&mut sh, "quit");
    assert!(!app.should_quit());
}

#[test]
fn save_and_close_writes_the_buffer_out() {
    // What a window closing under an unfinished edit must do: the
    // text wins over the gesture.
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    typ(&mut app, &mut sh, "rescued");
    let saved = app.save_pending_edit(&mut sh);
    assert!(saved, "there was something to save");
    assert!(!app.body_dirty());
    assert!(sh.vault.iter().any(|(_, d)| d.source().contains("rescued")));
}

#[test]
fn save_and_close_over_a_clean_editor_does_nothing() {
    let (_d, mut sh, mut app) = editing();
    assert!(!app.save_pending_edit(&mut sh));
}

// === absolute body scrolling (what a scrollbar drag needs) ===

#[test]
fn the_body_viewport_can_be_scrolled_to_a_line() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for _ in 0..60 {
        app.on_key(&mut sh, "enter", false, false, None);
    }
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_scroll_to(30, 10);
    assert_eq!(app.body_scroll_start(10), 30);
}

#[test]
fn scrolling_past_the_end_clamps_to_the_last_screen() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for _ in 0..19 {
        app.on_key(&mut sh, "enter", false, false, None);
    }
    app.on_key(&mut sh, "escape", false, false, None);
    // 21 lines, a 10-line viewport: the last first-line is 11.
    app.body_scroll_to(999, 10);
    assert_eq!(app.body_scroll_start(10), 11);
}

#[test]
fn a_body_that_fits_never_scrolls() {
    let (_d, _sh, mut app) = editing();
    app.body_scroll_to(999, 40);
    assert_eq!(app.body_scroll_start(40), 0);
}

// === resolving a link target against the vault ===
//
// `select_by_id` was the only way in, so org's other two link
// spellings — a fuzzy `[[Some Heading]]` and a `file:` cross-reference
// — had nothing to resolve against and came back "not in this vault".

fn linked() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("a.org"), "* Alpha\n* Beta\n").expect("write a");
    fs::write(dir.path().join("b.org"), "* Gamma\n").expect("write b");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The title of the row the outline is parked on.
fn selected_title(app: &ModalApp, sh: &Shell) -> String {
    app.rows(sh)[app.selected()].title.clone()
}

#[test]
fn a_headline_is_selectable_by_title() {
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(app.select_by_title(&sh, "Beta"));
    assert_eq!(selected_title(&app, &sh), "Beta");
}

#[test]
fn a_title_that_is_not_there_leaves_the_cursor_alone() {
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(app.select_by_title(&sh, "Beta"));
    assert!(!app.select_by_title(&sh, "Delta"));
    assert_eq!(selected_title(&app, &sh), "Beta", "unmoved");
}

#[test]
fn a_file_selects_its_first_headline() {
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(app.select_in_file(&sh, "b.org", None));
    assert_eq!(selected_title(&app, &sh), "Gamma");
}

#[test]
fn a_file_and_a_heading_selects_that_heading() {
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(app.select_in_file(&sh, "a.org", Some("Beta")));
    assert_eq!(selected_title(&app, &sh), "Beta");
}

#[test]
fn a_file_link_matches_on_the_trailing_path() {
    // Org writes `file:./b.org` and `file:notes/b.org` for the same
    // file depending on where the link lives.
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(app.select_in_file(&sh, "./b.org", None));
    assert_eq!(selected_title(&app, &sh), "Gamma");
}

#[test]
fn an_unknown_file_is_not_found() {
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(!app.select_in_file(&sh, "nowhere.org", None));
}

#[test]
fn a_heading_that_is_not_in_that_file_is_not_found() {
    let (_d, sh) = linked();
    let mut app = ModalApp::new(InputMode::Vim);
    assert!(!app.select_in_file(&sh, "b.org", Some("Alpha")));
}
