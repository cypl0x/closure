//! Q3-V4 — SCHEDULED and DEADLINE, with a calendar you can see.
//!
//! Planning a task was the one org verb with no way in at all: the
//! kernel has had `set-planning` since M2, and no shell ever called it.
//! Typing `<2026-07-30 Thu>` by hand is not planning a task — you have
//! to know what day that is.
//!
//! So: a date picker over the same month grid every calendar draws,
//! moved with the motion keys the rest of the app uses, committing
//! through the kernel command (undoable, I3) with the weekday org
//! itself would have written. A typed date still wins — including its
//! repeater, which no picker can express.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* TODO Ship it
:PROPERTIES:
:ID: 01HQPLAN00000000000000001
:END:
body
* TODO Already planned
SCHEDULED: <2026-01-02 Fri>
:PROPERTIES:
:ID: 01HQPLAN00000000000000002
:END:
body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    // The shells inject today; the core never reads a clock, so every
    // one of these is reproducible.
    app.set_today("2026-07-28");
    (dir, Shell::new(vault), app)
}

fn source(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("notes.org")).expect("read")
}

fn on_first(app: &mut ModalApp, shell: &Shell) {
    assert!(app.select_by_id(shell, "01HQPLAN00000000000000001"));
}

// === the picker opens where it should ===

#[test]
fn v4_schedule_opens_the_picker_on_today() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");

    assert_eq!(app.surface(), ModalSurface::DatePick);
    let grid = app.date_grid();
    assert_eq!(grid.selected, "2026-07-28", "today, when nothing is set");
    assert_eq!(grid.year, 2026);
    assert_eq!(grid.month, 7);
    assert_eq!(grid.field, "SCHEDULED");
}

#[test]
fn v4_the_picker_opens_on_the_date_the_headline_already_has() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQPLAN00000000000000002"));
    app.run(&mut shell, "schedule");
    assert_eq!(app.date_grid().selected, "2026-01-02");
}

#[test]
fn v4_the_month_grid_is_the_month() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    let grid = app.date_grid();

    // July 2026: 31 days, the 1st is a Wednesday.
    let days: Vec<u32> = grid.weeks.iter().flatten().flatten().copied().collect();
    assert_eq!(days.len(), 31, "every day once: {days:?}");
    assert_eq!(days.first(), Some(&1));
    assert_eq!(days.last(), Some(&31));
    assert!(
        grid.weeks.iter().all(|w| w.len() == 7),
        "seven columns per row: {:?}",
        grid.weeks
    );
    // Monday-first, so a Wednesday 1st leaves two blanks.
    assert_eq!(
        grid.weeks[0][..3],
        [None, None, Some(1)],
        "the 1st sits on Wednesday"
    );
}

// === moving in it ===

#[test]
fn v4_motions_move_the_day() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");

    app.on_key(&mut shell, "l", false, false, Some('l'));
    assert_eq!(app.date_grid().selected, "2026-07-29");
    app.on_key(&mut shell, "j", false, false, Some('j'));
    assert_eq!(app.date_grid().selected, "2026-08-05", "a week on");
    app.on_key(&mut shell, "k", false, false, Some('k'));
    assert_eq!(app.date_grid().selected, "2026-07-29");
    app.on_key(&mut shell, "h", false, false, Some('h'));
    assert_eq!(app.date_grid().selected, "2026-07-28");
}

#[test]
fn v4_a_month_step_lands_in_the_next_month() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");

    app.on_key(&mut shell, ">", false, false, Some('>'));
    assert_eq!(app.date_grid().selected, "2026-08-28");
    app.on_key(&mut shell, "<", false, false, Some('<'));
    assert_eq!(app.date_grid().selected, "2026-07-28");
}

#[test]
fn v4_a_month_step_from_the_31st_clamps_to_the_month_that_has_no_31st() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    for _ in 0..3 {
        app.on_key(&mut shell, "l", false, false, Some('l'));
    }
    assert_eq!(app.date_grid().selected, "2026-07-31");
    app.on_key(&mut shell, "<", false, false, Some('<'));
    assert_eq!(app.date_grid().selected, "2026-06-30", "June has 30 days");
}

#[test]
fn v4_dot_is_today() {
    let (_d, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    app.on_key(&mut shell, ">", false, false, Some('>'));
    app.on_key(&mut shell, ".", false, false, Some('.'));
    assert_eq!(app.date_grid().selected, "2026-07-28");
}

// === committing ===

#[test]
fn v4_enter_writes_the_stamp_org_would_have_written() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    app.on_key(&mut shell, "l", false, false, Some('l'));
    app.on_key(&mut shell, "enter", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(
        source(&dir).contains("SCHEDULED: <2026-07-29 Wed>"),
        "the file: {}",
        source(&dir)
    );
}

#[test]
fn v4_deadline_writes_a_deadline_and_keeps_the_schedule() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQPLAN00000000000000002"));
    app.run(&mut shell, "deadline");
    app.on_key(&mut shell, "enter", false, false, None);

    let src = source(&dir);
    assert!(src.contains("SCHEDULED: <2026-01-02 Fri>"), "{src}");
    assert!(src.contains("DEADLINE: <2026-07-28 Tue>"), "{src}");
}

#[test]
fn v4_a_typed_date_wins_over_the_grid() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    for c in "2026-12-24".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        source(&dir).contains("SCHEDULED: <2026-12-24 Thu>"),
        "{}",
        source(&dir)
    );
}

#[test]
fn v4_a_typed_repeater_survives() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    for c in "2026-12-24 +1w".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(
        source(&dir).contains("SCHEDULED: <2026-12-24 Thu +1w>"),
        "{}",
        source(&dir)
    );
}

#[test]
fn v4_a_typed_nonsense_date_is_refused_and_says_so() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    for c in "not-a-date".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::DatePick,
        "the picker stays open"
    );
    assert!(app.status().contains("date"), "status: {}", app.status());
    assert!(!source(&dir).contains("SCHEDULED: <not"), "nothing written");
}

#[test]
fn v4_x_clears_the_planning_it_was_opened_on() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQPLAN00000000000000002"));
    app.run(&mut shell, "schedule");
    app.on_key(&mut shell, "x", false, false, Some('x'));

    let src = source(&dir);
    assert!(!src.contains("SCHEDULED:"), "the stamp is gone: {src}");
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn v4_escape_writes_nothing() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    app.on_key(&mut shell, "l", false, false, Some('l'));
    app.on_key(&mut shell, "escape", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(!source(&dir).contains("SCHEDULED: <2026-07-29"));
}

#[test]
fn v4_planning_is_undoable_like_every_other_edit() {
    let (dir, mut shell, mut app) = fixture();
    on_first(&mut app, &shell);
    app.run(&mut shell, "schedule");
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(source(&dir).contains("SCHEDULED: <2026-07-28 Tue>"));

    app.run(&mut shell, "undo");
    assert!(
        !source(&dir).contains("SCHEDULED: <2026-07-28"),
        "undo took it back: {}",
        source(&dir)
    );
}

#[test]
fn v4_with_nothing_selected_the_picker_does_not_open() {
    let (_d, mut shell, mut app) = fixture();
    app.clear_selection();
    app.run(&mut shell, "schedule");
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(!app.status().is_empty(), "it says why");
}
