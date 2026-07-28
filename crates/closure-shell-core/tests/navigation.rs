//! Getting from a search hit, or a click, to the thing itself.
//!
//! The outline's row list is *filtered* while the search overlay is
//! open, so the cursor is an index into the results. Accepting the
//! search cleared the query — which unfiltered the list — and left the
//! index where it was, so Enter on the third hit selected the third row
//! of the whole vault. The selection has to be carried across by id,
//! because the id is the only thing both lists agree on.
//!
//! And Enter on a row in the outline said what the row was in the
//! status line, which is not what Enter means anywhere else: it opens
//! the thing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* Alpha one
:PROPERTIES:
:ID: 01HQNAV000000000000000001
:END:
* Beta two
:PROPERTIES:
:ID: 01HQNAV000000000000000002
:END:
* Gamma three
:PROPERTIES:
:ID: 01HQNAV000000000000000003
:END:
Gamma has a body.
* Delta four
:PROPERTIES:
:ID: 01HQNAV000000000000000004
:END:
";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

fn app() -> (TempDir, Shell, ModalApp) {
    let (d, sh) = shell();
    (d, sh, ModalApp::new(InputMode::Doom))
}

fn type_query(app: &mut ModalApp, sh: &mut Shell, q: &str) {
    app.run(sh, "search-start");
    for c in q.chars() {
        app.on_key(sh, "x", false, false, Some(c));
    }
}

fn selected_title(app: &ModalApp, sh: &Shell) -> String {
    app.rows(sh)
        .get(app.selected())
        .map(|r| r.title.clone())
        .unwrap_or_default()
}

#[test]
fn accepting_a_search_selects_the_hit_in_the_tree() {
    let (_d, mut sh, mut app) = app();
    type_query(&mut app, &mut sh, "gamma");
    let hit = selected_title(&app, &sh);
    assert!(hit.contains("Gamma"), "the search found it: {hit}");

    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(
        selected_title(&app, &sh).contains("Gamma"),
        "and the outline is on it, not on whatever row that index is \
         in the unfiltered list: {}",
        selected_title(&app, &sh)
    );
    assert!(app.selection_active());
}

#[test]
fn a_search_moved_through_with_the_chords_still_lands_on_the_hit() {
    let (_d, mut sh, mut app) = app();
    // Everything matches the empty-ish query; move down twice, then
    // accept whatever is under the cursor.
    type_query(&mut app, &mut sh, "a");
    app.on_key(&mut sh, "j", true, false, Some('j'));
    let hit = selected_title(&app, &sh);
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(selected_title(&app, &sh), hit);
}

#[test]
fn abandoning_a_search_leaves_the_outline_where_it_was() {
    // Esc is "never mind", so it must not move the cursor either.
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000004");
    let before = selected_title(&app, &sh);
    type_query(&mut app, &mut sh, "gamma");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(selected_title(&app, &sh), before, "back where we started");
}

#[test]
fn enter_on_a_row_opens_its_body() {
    // Enter reported the row in the status line. Everywhere else in the
    // app — and everywhere else in every app — it opens the thing.
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(
        app.body_buffer().contains("Gamma has a body"),
        "on the row that was selected: {:?}",
        app.body_buffer()
    );
}

#[test]
fn enter_from_a_search_hit_opens_that_hit() {
    // The two halves together: find it, press Enter twice, be editing
    // the thing you searched for.
    let (_d, mut sh, mut app) = app();
    type_query(&mut app, &mut sh, "gamma");
    app.on_key(&mut sh, "enter", false, false, None);
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(app.body_buffer().contains("Gamma has a body"));
}

// === the "/" block menu ===

#[test]
fn ctrl_j_and_k_move_through_the_slash_menu() {
    // Reported for Doom mode: the menu answered the arrows and nothing
    // else, so in a modal mode it was mouse-and-arrows only.
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.on_key(&mut sh, "i", false, false, Some('i')); // INSERT
    app.on_key(&mut sh, "/", false, false, Some('/'));
    assert!(app.slash_query().is_some(), "the menu opened");
    let start = app.slash_cursor();
    app.on_key(&mut sh, "j", true, false, Some('j'));
    assert_eq!(app.slash_cursor(), start + 1, "C-j moves down");
    app.on_key(&mut sh, "k", true, false, Some('k'));
    assert_eq!(app.slash_cursor(), start, "C-k moves back");
}

#[test]
fn a_plain_j_in_the_slash_menu_is_still_typed() {
    // The menu filters as you type: `j` is a letter of the query, and
    // only the chord is navigation.
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.on_key(&mut sh, "i", false, false, Some('i'));
    app.on_key(&mut sh, "/", false, false, Some('/'));
    app.on_key(&mut sh, "j", false, false, Some('j'));
    assert!(
        app.body_buffer().contains("/j"),
        "typed into the buffer: {:?}",
        app.body_buffer()
    );
}

// === the buffer remembers where you were ===

#[test]
fn reopening_a_body_puts_the_cursor_back() {
    // Closing a body and opening it again started at byte zero, so any
    // edit deeper in a long note meant navigating there again.
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.body_set_cursor(6);
    assert_eq!(app.body_cursor(), (0, 6));
    app.on_key(&mut sh, "escape", false, false, None); // clean buffer, leaves

    app.run(&mut sh, "edit-body");
    assert_eq!(app.body_cursor(), (0, 6), "back where the cursor was left");
}

#[test]
fn each_headline_remembers_its_own_cursor() {
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.body_set_cursor(9);
    app.on_key(&mut sh, "escape", false, false, None);

    // A different headline is its own place, not the one we just left.
    app.select_by_id(&sh, "01HQNAV000000000000000001");
    app.run(&mut sh, "edit-body");
    assert_eq!(
        app.body_cursor(),
        (0, 0),
        "an unvisited body opens at the top"
    );
}

#[test]
fn a_remembered_cursor_past_the_end_is_clamped() {
    // The body can shrink between visits — from another window, a sync
    // round, or an undo — and a stale offset must not panic or land
    // outside the buffer (I5).
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.body_set_cursor(15);
    app.on_key(&mut sh, "escape", false, false, None);

    let id = closure_core::BlockId::from_existing("01HQNAV000000000000000003");
    sh.set_body(&id, "hi\n").expect("shrunk");
    app.invalidate_rows();
    app.run(&mut sh, "edit-body");
    let (_line, col) = app.body_cursor();
    assert!(col <= 2, "clamped into the shorter body, got {col}");
}

// === completion ===

#[test]
fn completion_offers_words_from_the_vault_in_the_body_editor() {
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000001");
    app.run(&mut sh, "edit-body");
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "Gam".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert!(
        app.completion_should_popup(&sh),
        "`Gam` prefixes `Gamma`, which is in the vault"
    );
}

#[test]
fn completion_works_in_the_full_window_editor_too() {
    // Reported as "autocompletion is not working": the check was
    // pinned to `EditBody`, and the editor *view* is `EditFile` — a
    // different surface holding the same buffer, so the popup could
    // never arm there.
    let (_d, mut sh, mut app) = app();
    app.set_view(closure_shell_core::ViewMode::Editor, &sh);
    assert_eq!(app.surface(), ModalSurface::EditFile);
    app.on_key(&mut sh, "i", false, false, Some('i')); // INSERT
    for c in "Gam".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert!(
        app.completion_should_popup(&sh),
        "the same buffer, the same completion"
    );
}

#[test]
fn cycling_completion_applies_a_candidate() {
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000001");
    app.run(&mut sh, "edit-body");
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "Gam".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.open_completion_popup(&sh);
    app.on_key(&mut sh, "n", true, false, None); // C-n
    assert!(
        app.body_buffer().contains("Gamma"),
        "the candidate went into the buffer: {:?}",
        app.body_buffer()
    );
}

// === SPC s s in a buffer searches the buffer ===

#[test]
fn search_from_inside_the_editor_searches_the_buffer() {
    // Doom's `SPC s s` is `search-buffer` — swiper over the thing you
    // are looking at. Bound to the vault-wide headline search, it threw
    // you out of the buffer to look somewhere else entirely.
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.run(&mut sh, "search-start");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "still in the buffer, not in the outline's search"
    );
    assert!(
        app.body_search_prompt().is_some(),
        "with the buffer's own search line open"
    );
}

#[test]
fn a_buffer_search_finds_a_line_in_it() {
    let (_d, mut sh, mut app) = app();
    app.select_by_id(&sh, "01HQNAV000000000000000003");
    app.run(&mut sh, "edit-body");
    app.run(&mut sh, "search-start");
    for c in "body".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(
        app.body_cursor().1 > 0 || app.body_cursor().0 > 0,
        "the cursor moved to the hit"
    );
}

#[test]
fn search_from_the_outline_still_searches_the_vault() {
    let (_d, mut sh, mut app) = app();
    app.run(&mut sh, "search-start");
    assert_eq!(app.surface(), ModalSurface::Search);
}
