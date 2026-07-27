//! Capture, and what happens to the outline around it.
//!
//! Capture wrote the file and stopped there: the row list is memoised
//! against the vault revision and capture was the one mutation that
//! never moved it, so the item you had just typed was not on screen.
//! Even once it appeared, it appeared *somewhere* — always at the top
//! level of `inbox.org`, never under whatever you were looking at, and
//! never selected, so the next thing you did happened to the wrong
//! headline.
//!
//! Org captures into a target; the target people actually mean is "the
//! thing I am looking at", with the top level as the fallback for when
//! they are looking at nothing. Escape is how you say "nothing".

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* Project
:PROPERTIES:
:ID: 01HQCAP000000000000000001
:END:
** Existing child
:PROPERTIES:
:ID: 01HQCAP000000000000000002
:END:
* Other
:PROPERTIES:
:ID: 01HQCAP000000000000000003
:END:
";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Type `title` into the capture overlay and accept it.
fn capture(app: &mut ModalApp, sh: &mut Shell, title: &str) {
    app.run(sh, "capture-start");
    assert_eq!(app.surface(), ModalSurface::Capture);
    for c in title.chars() {
        app.on_key(sh, "x", false, false, Some(c));
    }
    app.on_key(sh, "enter", false, false, None);
}

fn titles(app: &ModalApp, sh: &Shell) -> Vec<String> {
    app.rows(sh).into_iter().map(|r| r.title).collect()
}

#[test]
fn a_captured_item_is_on_screen_without_anything_else_happening() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    let before = app.rows(&sh).len();
    capture(&mut app, &mut sh, "Buy milk");
    let after = titles(&app, &sh);
    assert_eq!(after.len(), before + 1, "{after:?}");
    assert!(
        after.iter().any(|t| t.contains("Buy milk")),
        "the row list rebuilt: {after:?}"
    );
}

#[test]
fn a_captured_item_is_the_selected_one() {
    // Whatever you do next — open it, tag it, type a body — happens to
    // the thing you just made, not to whatever the cursor was on.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    capture(&mut app, &mut sh, "Buy milk");
    let rows = app.rows(&sh);
    let selected = &rows[app.selected()];
    assert!(
        selected.title.contains("Buy milk"),
        "selected {:?} of {:?}",
        selected.title,
        titles(&app, &sh)
    );
}

#[test]
fn capture_files_the_item_under_the_selected_headline() {
    // "Capture this under what I am looking at" is what an outliner is
    // for; a flat inbox is the fallback, not the rule.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQCAP000000000000000001"); // Project
    let parent_level = app.rows(&sh)[app.selected()].level;

    capture(&mut app, &mut sh, "Sub task");
    let rows = app.rows(&sh);
    let new = rows
        .iter()
        .find(|r| r.title.contains("Sub task"))
        .expect("captured row");
    assert_eq!(
        new.level,
        parent_level + 1,
        "a child of the selection, not a sibling of the file"
    );
    let parent_at = rows
        .iter()
        .position(|r| r.title.contains("Project"))
        .expect("parent");
    let new_at = rows
        .iter()
        .position(|r| r.title.contains("Sub task"))
        .expect("new");
    assert!(new_at > parent_at, "and it sits inside that subtree");
}

#[test]
fn escape_clears_the_selection_and_capture_goes_top_level() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQCAP000000000000000001");
    app.on_key(&mut sh, "escape", false, false, None);
    assert!(
        !app.selection_active(),
        "escape is how you say `I am looking at nothing`"
    );

    capture(&mut app, &mut sh, "Loose thought");
    let rows = app.rows(&sh);
    let new = rows
        .iter()
        .find(|r| r.title.contains("Loose thought"))
        .expect("captured row");
    assert_eq!(new.level, 1, "top level when nothing is selected");
}

#[test]
fn capturing_makes_the_selection_active_again() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, "escape", false, false, None);
    capture(&mut app, &mut sh, "Loose thought");
    assert!(
        app.selection_active(),
        "the new item is selected, so there is a selection again"
    );
}

#[test]
fn a_motion_reactivates_the_selection() {
    // Escape means "nothing is selected"; the next `j` means "this is".
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, "escape", false, false, None);
    assert!(!app.selection_active());
    app.on_key(&mut sh, "j", false, false, Some('j'));
    assert!(app.selection_active(), "moving selects");
}

#[test]
fn a_captured_child_survives_a_reload_from_disk() {
    // The files are the API (I1): what the outline shows has to be
    // what a second reader of the directory would parse.
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQCAP000000000000000001");
    capture(&mut app, &mut sh, "Sub task");

    let reopened = Vault::open(dir.path()).expect("reopen");
    let fresh = Shell::new(reopened);
    let rows = ModalApp::new(InputMode::Doom).rows(&fresh);
    let new = rows
        .iter()
        .find(|r| r.title.contains("Sub task"))
        .expect("on disk");
    assert_eq!(new.level, 2, "written as a child, not just shown as one");
}

// === promotion and what the outline says about it ===

#[test]
fn demoting_a_headline_shows_up_in_the_rows() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQCAP000000000000000003"); // Other, level 1
    app.run(&mut sh, "demote");
    let rows = app.rows(&sh);
    let row = rows
        .iter()
        .find(|r| r.title.contains("Other"))
        .expect("still there");
    assert_eq!(row.level, 2, "the outline shows the new level");
}

#[test]
fn promoting_a_top_level_headline_says_why_it_cannot() {
    // `Promote` refuses at level 1 — there is no level 0. The refusal
    // used to be dropped on the floor (`let _ = …`), so the key did
    // nothing and said nothing, which reads as "the UI did not
    // refresh".
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQCAP000000000000000003"); // level 1
    app.run(&mut sh, "promote");
    assert!(
        !app.status().is_empty(),
        "a refusal has to reach the status line"
    );
    assert_eq!(
        app.rows(&sh)
            .iter()
            .find(|r| r.title.contains("Other"))
            .expect("still there")
            .level,
        1,
        "and nothing moved"
    );
}

#[test]
fn promoting_a_child_lifts_it_in_the_rows() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQCAP000000000000000002"); // Existing child, level 2
    app.run(&mut sh, "promote");
    let rows = app.rows(&sh);
    let row = rows
        .iter()
        .find(|r| r.title.contains("Existing child"))
        .expect("still there");
    assert_eq!(row.level, 1);
}

// === the overlays as text fields ===

#[test]
fn the_capture_field_answers_to_the_readline_chords() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "capture-start");
    for c in "Call Leon tomorrow".chars() {
        app.on_key(&mut sh, "x", false, false, Some(c));
    }
    app.on_key(&mut sh, "w", true, false, None); // C-w
    assert_eq!(app.capture_buffer(), "Call Leon ");
    app.on_key(&mut sh, "a", true, false, None); // C-a
    assert_eq!(app.capture_cursor(), 0);
    app.on_key(&mut sh, "x", false, false, Some('!'));
    assert_eq!(
        app.capture_buffer(),
        "!Call Leon ",
        "typing happens at the cursor, not at the end"
    );
}

#[test]
fn a_full_stop_in_a_capture_is_just_a_full_stop() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "capture-start");
    for c in "Ring Leon. Then rest".chars() {
        app.on_key(&mut sh, "x", false, false, Some(c));
    }
    assert_eq!(app.capture_buffer(), "Ring Leon. Then rest");
}

#[test]
fn ctrl_j_and_k_move_through_search_results() {
    // The arrows moved the cursor and the chords did not, which in a
    // modal app is the wrong way round.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "search-start");
    let start = app.selected();
    app.on_key(&mut sh, "j", true, false, Some('j'));
    assert_eq!(app.selected(), start + 1, "C-j moves down");
    app.on_key(&mut sh, "k", true, false, Some('k'));
    assert_eq!(app.selected(), start, "C-k moves back");
    assert_eq!(app.query(), "", "and neither typed a letter");
}
